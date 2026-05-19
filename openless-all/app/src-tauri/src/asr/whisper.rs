//! Batch Whisper ASR client — collects PCM in a buffer, then POSTs a WAV file
//! to any OpenAI-compatible `/audio/transcriptions` endpoint on session end.

use anyhow::{Context, Result};
use parking_lot::Mutex;

use crate::asr::wav::encode_wav_16k_mono;
use crate::asr::RawTranscript;

/// Whisper の `prompt` パラメータの安全側上限（文字数）。
///
/// OpenAI / Groq の Audio Transcriptions API は `prompt` を 244 トークンまで
/// 受け付ける。トークナイザは BPE で言語によって 1 token あたりの文字数が
/// 異なる：英語は ~4 chars/token、日本語・中国語は最悪 ~1 char/token。
/// CJK ユーザーが安全に収まるよう、文字数で 240 を上限にする。
pub const PROMPT_CHAR_BUDGET: usize = 240;

/// 区切り文字（ASCII）。Whisper のトークナイザはどの言語でも安定して扱える。
const PROMPT_SEPARATOR: &str = ", ";

pub struct WhisperBatchASR {
    api_key: String,
    base_url: String,
    model: String,
    /// 任意のプロンプト（語彙ヒント等）。空文字や空白のみは送信しない。
    /// `None` ＝ プロンプト無し（既存挙動）。
    prompt: Option<String>,
    buffer: Mutex<Vec<u8>>,
}

impl WhisperBatchASR {
    pub fn new(
        api_key: String,
        base_url: String,
        model: String,
        prompt: Option<String>,
    ) -> Self {
        Self {
            api_key,
            base_url,
            model,
            prompt,
            buffer: Mutex::new(Vec::new()),
        }
    }

    /// Stop collecting audio, encode the buffer as WAV, and POST to the
    /// Whisper transcriptions endpoint.
    ///
    /// 失败时**保留** PCM buffer，让上层有机会重试或在历史中至少留一个失败记录；
    /// 之前的实现一进函数就 `mem::take` 把 buffer 清空，凭证错或网络中断都会
    /// 让用户的录音直接消失。
    pub async fn transcribe(&self) -> Result<RawTranscript> {
        // clone 而不是 take：~30s 16 kHz 16-bit 音频 ≈ 960 KB，会话末调用一次，可接受。
        let pcm = self.buffer.lock().clone();
        if pcm.is_empty() {
            return Ok(RawTranscript {
                text: String::new(),
                duration_ms: 0,
            });
        }

        let result = self.transcribe_inner(&pcm).await;
        // 仅在成功路径上才清 buffer。失败时 PCM 还在，coordinator 拿到 Err 但
        // 用户重新触发 stop 时仍能再发一次，或日后增加重试入口时复用。
        if result.is_ok() {
            self.buffer.lock().clear();
        }
        result
    }

    async fn transcribe_inner(&self, pcm: &[u8]) -> Result<RawTranscript> {
        // 16 kHz mono 16-bit: 2 bytes per sample.
        let duration_ms = (pcm.len() as u64 / 2) * 1000 / 16_000;

        if self.api_key.is_empty() {
            anyhow::bail!("Whisper API key missing");
        }

        let samples: Vec<i16> = pcm
            .chunks_exact(2)
            .map(|chunk| i16::from_le_bytes([chunk[0], chunk[1]]))
            .collect();
        let wav = encode_wav_16k_mono(&samples);
        let base_url = self.base_url.trim_end_matches('/');
        let url = format!("{}/audio/transcriptions", base_url);

        let wav_part = reqwest::multipart::Part::bytes(wav)
            .file_name("audio.wav")
            .mime_str("audio/wav")
            .context("set MIME type")?;
        let mut form = reqwest::multipart::Form::new()
            .part("file", wav_part)
            .text("model", self.model.clone())
            // verbose_json でセグメント単位のメタデータ（no_speech_prob /
            // avg_logprob / compression_ratio）を取得する。これを使って
            // Whisper の幻聴（無音・ノイズ区間での作話）セグメントを捨てる。
            .text("response_format", "verbose_json")
            // 文字起こしは決定論的タスク。temperature を明示 0 にして
            // ランダムな作話の余地を減らす。
            .text("temperature", "0");

        // `prompt` は空文字を送らない：OpenAI 互換実装によっては空文字でエラーに
        // なるリスクがある（Groq は許容するが防御的にスキップ）。`trim()` で
        // 空白のみのケースも除外。
        if let Some(prompt) = self.prompt.as_ref() {
            let trimmed = prompt.trim();
            if !trimmed.is_empty() {
                form = form.text("prompt", trimmed.to_string());
            }
        }

        let client = reqwest::Client::new();
        let resp = client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .multipart(form)
            .send()
            .await
            .context("Whisper HTTP request failed")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Whisper API error {}: {}", status, body);
        }

        let json: serde_json::Value = resp.json().await.context("parse Whisper response")?;
        let text = extract_confident_text(&json);
        // 辞書プロンプトの echo（ユーザーが言っていない辞書語の羅列）を除去。
        let text = strip_prompt_echo(&text, self.prompt.as_deref());

        Ok(RawTranscript { text, duration_ms })
    }

    pub fn cancel(&self) {
        self.buffer.lock().clear();
    }
}

impl crate::recorder::AudioConsumer for WhisperBatchASR {
    fn consume_pcm_chunk(&self, pcm: &[u8]) {
        self.buffer.lock().extend_from_slice(pcm);
    }
}

/// verbose_json レスポンスから、幻聴と思われるセグメントを除いた本文を組む。
///
/// Whisper は無音・小音・ノイズ区間で「もっともらしいが言っていない」テキストを
/// 生成する既知の欠陥（hallucination）がある。録音の前後の沈黙やマイクのノイズが
/// 無関係な単語に化けるのがこれ。verbose_json の各セグメントが持つ
/// `no_speech_prob` / `avg_logprob` / `compression_ratio` を見て、明らかに
/// 発話でないセグメントを捨てる。
///
/// 判定（いずれかに該当したら捨てる）:
/// - `no_speech_prob > 0.6` かつ `avg_logprob < -0.5`
///   → 無音確率が高く信頼度も低い。沈黙を作話したセグメント。
/// - `compression_ratio > 2.4`
///   → 同一フレーズの反復幻聴（Whisper 標準の閾値）。
/// - `avg_logprob < -1.0`
///   → 信頼度が極端に低い。ノイズを単語化したセグメント。
///
/// 実発話を誤って捨てるのが最悪なので、閾値は保守的に設定している。
/// `segments` が無いレスポンスでは従来どおり `text` をそのまま使う。
fn extract_confident_text(json: &serde_json::Value) -> String {
    let Some(segments) = json.get("segments").and_then(|s| s.as_array()) else {
        return json["text"].as_str().unwrap_or("").trim().to_string();
    };

    let mut kept = String::new();
    for seg in segments {
        let text = seg.get("text").and_then(|t| t.as_str()).unwrap_or("");
        if text.trim().is_empty() {
            continue;
        }
        let no_speech = seg
            .get("no_speech_prob")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        let avg_logprob = seg
            .get("avg_logprob")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        let compression = seg
            .get("compression_ratio")
            .and_then(|v| v.as_f64())
            .unwrap_or(1.0);

        let is_hallucination = (no_speech > 0.6 && avg_logprob < -0.5)
            || compression > 2.4
            || avg_logprob < -1.0;
        if is_hallucination {
            log::warn!(
                "[whisper] 幻聴セグメントを除外: no_speech={:.2} avg_logprob={:.2} compression={:.2} text={:?}",
                no_speech,
                avg_logprob,
                compression,
                text.trim()
            );
            continue;
        }
        kept.push_str(text);
    }

    let kept = kept.trim().to_string();
    if kept.is_empty() {
        // 全セグメントが除外された（＝ほぼ無音録音）。フォールバックで
        // 生 text を返すと幻聴を拾い直すので、空のまま返す。上位は空転写を
        // 「何も話していない」として無害に扱う。
        return String::new();
    }
    kept
}

/// 2 つの文字列の最長共通部分文字列を返す `(a 内開始位置, 長さ)`。
/// 長さ 0 のときは `(0, 0)`。
fn longest_common_substring(a: &[char], b: &[char]) -> (usize, usize) {
    if a.is_empty() || b.is_empty() {
        return (0, 0);
    }
    let mut prev = vec![0usize; b.len() + 1];
    let mut best_len = 0usize;
    let mut best_end_in_a = 0usize;
    for i in 1..=a.len() {
        let mut curr = vec![0usize; b.len() + 1];
        for j in 1..=b.len() {
            if a[i - 1] == b[j - 1] {
                curr[j] = prev[j - 1] + 1;
                if curr[j] > best_len {
                    best_len = curr[j];
                    best_end_in_a = i;
                }
            }
        }
        prev = curr;
    }
    (best_end_in_a - best_len, best_len)
}

/// Whisper が `prompt`（辞書語の `", "` 連結）を出力に echo（漏出）させた
/// 断片を取り除く。
///
/// Whisper には prompt の内容を書き起こしに紛れ込ませる既知の欠陥があり、
/// ユーザーが言っていない辞書語が「片沼ほとり, ADOS,」のようにカンマ込みで
/// 出力されることがある。prompt は既知文字列なので、出力と prompt の最長共通
/// 部分文字列を取り、それが **カンマ様の区切りを含む**（＝辞書語の羅列）なら
/// echo とみなして除去する。カンマを含まない＝単独語の一致は、ユーザーが実際に
/// その語を言った正当なケースと区別できないため除去しない。
fn strip_prompt_echo(text: &str, prompt: Option<&str>) -> String {
    let Some(prompt) = prompt else {
        return text.to_string();
    };
    let prompt = prompt.trim();
    if prompt.is_empty() {
        return text.to_string();
    }
    let prompt_chars: Vec<char> = prompt.chars().collect();
    let mut text_chars: Vec<char> = text.chars().collect();

    const MIN_ECHO_LEN: usize = 5;
    let is_comma = |c: char| matches!(c, ',' | '，' | '、');
    let is_sep = |c: char| c.is_whitespace() || is_comma(c);

    // 複数の echo 断片がありうるので、見つからなくなるまで繰り返す。
    loop {
        let (start, len) = longest_common_substring(&text_chars, &prompt_chars);
        if len < MIN_ECHO_LEN {
            break;
        }
        let frag = &text_chars[start..start + len];
        if !frag.iter().copied().any(is_comma) {
            // カンマを含まない一致は単独語。正当な発話の可能性があり除去しない。
            break;
        }
        log::warn!(
            "[whisper] prompt echo を除去: {:?}",
            frag.iter().collect::<String>()
        );
        text_chars.drain(start..start + len);

        // 除去で区切りが二重化したときだけ畳む：除去点の前後がどちらも
        // 区切り（カンマ/空白）なら、後ろ側の区切り連続を削って 1 つにする。
        // 片側だけが区切りの場合はユーザーが実際に打った句読点なので残す。
        let head_sep = start > 0 && is_sep(text_chars[start - 1]);
        let tail_sep = start < text_chars.len() && is_sep(text_chars[start]);
        if head_sep && tail_sep {
            while start < text_chars.len() && is_sep(text_chars[start]) {
                text_chars.remove(start);
            }
        }
    }

    // 先頭・末尾に残った区切りを除去（echo が文頭/文末にあった場合）。
    let mut result: Vec<char> = text_chars;
    while result.first().is_some_and(|&c| is_sep(c)) {
        result.remove(0);
    }
    while result.last().is_some_and(|&c| is_sep(c)) {
        result.pop();
    }
    result.iter().collect()
}

/// 用户辞書の有効フレーズから Whisper の `prompt` パラメータを組み立てる。
///
/// Whisper は `prompt` で語彙ヒント / スタイル文脈を渡せる：固有名詞・専門
/// 用語の表記揺れを抑え、ASR 段階で正しい綴り（漢字選択を含む）に偏らせる。
/// 既存の dictionary 機能はこれまで Volcengine ASR と Polish LLM のみに渡って
/// いて、Whisper 互換プロバイダ（whisper / siliconflow / zhipu / groq）には
/// 流れていなかった。本関数で同じエントリを Whisper にも届ける。
///
/// # 仕様
///
/// - 空白のみのフレーズは除外
/// - 区切りは `, `
/// - 末尾に `.` を付与して「文の終わり」を Whisper に明示（モデルがプロンプト
///   を続きと誤解して書き起こし冒頭に混入するのを抑える）
/// - 文字数が `PROMPT_CHAR_BUDGET` を超えるエントリは**スキップ**して次に
///   進む（途中で打ち切らない）。これにより「先頭に長文 1 件があると残りが
///   全部捨てられる」現象を回避でき、登録順を保ちつつ収まるエントリを最大化
///   できる。
/// - 入力が空、または有効フレーズが 0 件の場合は `None` を返す。Optional に
///   することで「プロンプト無し」と「空文字プロンプト」を呼び出し側で区別
///   する必要をなくす。
pub fn build_prompt_from_phrases(phrases: &[String]) -> Option<String> {
    let mut included: Vec<&str> = Vec::new();
    let mut total_chars: usize = 0;

    for phrase in phrases {
        let trimmed = phrase.trim();
        if trimmed.is_empty() {
            continue;
        }
        let phrase_chars = trimmed.chars().count();
        let added = if included.is_empty() {
            phrase_chars
        } else {
            PROMPT_SEPARATOR.chars().count() + phrase_chars
        };
        // 末尾の "." 1 文字も予約。
        if total_chars + added + 1 > PROMPT_CHAR_BUDGET {
            continue;
        }
        included.push(trimmed);
        total_chars += added;
    }

    if included.is_empty() {
        return None;
    }
    let mut s = included.join(PROMPT_SEPARATOR);
    s.push('.');
    Some(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_prompt_echo_removes_comma_joined_dictionary_run() {
        let prompt = "梁山泊, 片沼ほとり, ADOS, TRC.";
        // 文中に prompt の断片が echo された
        let text = "逆に言うと、片沼ほとり, ADOS, をこっちに移す";
        let out = strip_prompt_echo(text, Some(prompt));
        assert!(!out.contains("ADOS"), "echo が残っている: {out}");
        assert!(out.contains("逆に言うと"));
        assert!(out.contains("こっちに移す"));
    }

    #[test]
    fn strip_prompt_echo_at_start_trims_leading_separators() {
        let prompt = "梁山泊, 片沼ほとり, TRC.";
        let text = "梁山泊, 片沼ほとり, 実際に話した内容";
        let out = strip_prompt_echo(text, Some(prompt));
        assert_eq!(out, "実際に話した内容");
    }

    #[test]
    fn strip_prompt_echo_keeps_legit_single_word() {
        // カンマを含まない単独一致は、ユーザーが実際にその語を言った可能性が
        // あるので除去しない。
        let prompt = "梁山泊, 片沼ほとり, TRC.";
        let text = "梁山泊について話します";
        assert_eq!(
            strip_prompt_echo(text, Some(prompt)),
            "梁山泊について話します"
        );
    }

    #[test]
    fn strip_prompt_echo_no_prompt_is_noop() {
        assert_eq!(strip_prompt_echo("普通の文章です", None), "普通の文章です");
    }

    #[test]
    fn build_prompt_returns_none_for_empty_input() {
        assert_eq!(build_prompt_from_phrases(&[]), None);
    }

    #[test]
    fn build_prompt_returns_none_when_all_phrases_blank() {
        let phrases = vec!["".to_string(), "   ".to_string(), "\t\n".to_string()];
        assert_eq!(build_prompt_from_phrases(&phrases), None);
    }

    #[test]
    fn build_prompt_single_phrase() {
        let phrases = vec!["梁山泊".to_string()];
        assert_eq!(build_prompt_from_phrases(&phrases), Some("梁山泊.".to_string()));
    }

    #[test]
    fn build_prompt_joins_with_comma_and_appends_period() {
        let phrases = vec![
            "梁山泊".to_string(),
            "片沼ほとり".to_string(),
            "TRC".to_string(),
        ];
        assert_eq!(
            build_prompt_from_phrases(&phrases),
            Some("梁山泊, 片沼ほとり, TRC.".to_string())
        );
    }

    #[test]
    fn build_prompt_trims_each_phrase() {
        let phrases = vec!["  梁山泊  ".to_string(), "\tTRC\n".to_string()];
        assert_eq!(
            build_prompt_from_phrases(&phrases),
            Some("梁山泊, TRC.".to_string())
        );
    }

    #[test]
    fn build_prompt_skips_blank_entries_in_middle() {
        let phrases = vec![
            "alpha".to_string(),
            "".to_string(),
            "   ".to_string(),
            "beta".to_string(),
        ];
        assert_eq!(
            build_prompt_from_phrases(&phrases),
            Some("alpha, beta.".to_string())
        );
    }

    #[test]
    fn build_prompt_truncates_overflow_but_keeps_short_entries_after_long_one() {
        // 先頭に 250 文字の長文 → 単独で予算超過 → スキップ。続く短いエントリは
        // 採用される。「途中で break しない」契約の検証。
        let long = "あ".repeat(250);
        let phrases = vec![long.clone(), "梁山泊".to_string(), "TRC".to_string()];
        let prompt = build_prompt_from_phrases(&phrases).expect("non-empty");
        assert!(!prompt.contains(&long), "long phrase must be dropped");
        assert!(prompt.contains("梁山泊"));
        assert!(prompt.contains("TRC"));
        assert!(prompt.ends_with('.'));
    }

    #[test]
    fn build_prompt_respects_char_budget() {
        // 6 文字 × 50 件 = 300 文字（区切り込みでさらに増える）→ 予算超過分は捨てる。
        let phrases: Vec<String> = (0..50).map(|i| format!("word{:02}", i)).collect();
        let prompt = build_prompt_from_phrases(&phrases).expect("non-empty");
        assert!(
            prompt.chars().count() <= PROMPT_CHAR_BUDGET,
            "prompt length {} exceeds budget {}",
            prompt.chars().count(),
            PROMPT_CHAR_BUDGET
        );
        assert!(prompt.ends_with('.'));
    }

    #[test]
    fn build_prompt_includes_first_entries_when_truncating_in_order() {
        // 順序保証：登録順の早いものから入る。後続が落ちる。
        let phrases: Vec<String> = (0..100).map(|i| format!("entry{:03}", i)).collect();
        let prompt = build_prompt_from_phrases(&phrases).expect("non-empty");
        assert!(prompt.contains("entry000"));
        assert!(prompt.contains("entry001"));
        // 100 件 × 8 文字以上は確実に予算超過 → 末尾は入らない
        assert!(!prompt.contains("entry099"));
    }
}
