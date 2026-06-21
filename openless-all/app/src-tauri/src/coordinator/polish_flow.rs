//! Polish / translate orchestration extracted from `coordinator.rs`
//! (behavior-preserving move).
//!
//! The streaming/one-shot polish entry points and the polish+translate combiner.
//! References parent items via `use super::*;`; `pub(super)` so the parent and
//! sibling submodules (e.g. `dictation`) reach them through `use polish_flow::*;`.

use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::polish::LLMError;

use super::*;

mod model_fallback {
    use parking_lot::Mutex;

    const RATE_LIMIT_COOLDOWN_SECS: i64 = 300;

    struct TemporaryBackoff {
        key: String,
        until: i64,
    }

    struct State {
        day: i64,
        daily_exhausted: Vec<String>,
        temporary_backoffs: Vec<TemporaryBackoff>,
    }

    static STATE: Mutex<State> = Mutex::new(State {
        day: 0,
        daily_exhausted: Vec::new(),
        temporary_backoffs: Vec::new(),
    });

    fn current_utc_day() -> i64 {
        chrono::Utc::now().timestamp().div_euclid(86_400)
    }

    fn current_unix_secs() -> i64 {
        chrono::Utc::now().timestamp()
    }

    fn roll_day(state: &mut State) {
        let today = current_utc_day();
        if state.day != today {
            state.day = today;
            state.daily_exhausted.clear();
        }
        let now = current_unix_secs();
        state.temporary_backoffs.retain(|b| b.until > now);
    }

    pub fn is_exhausted(key: &str) -> bool {
        let mut state = STATE.lock();
        roll_day(&mut state);
        state.daily_exhausted.iter().any(|m| m == key)
            || state.temporary_backoffs.iter().any(|b| b.key == key)
    }

    pub fn mark_daily_exhausted(key: &str) {
        let mut state = STATE.lock();
        roll_day(&mut state);
        if !state.daily_exhausted.iter().any(|m| m == key) {
            state.daily_exhausted.push(key.to_string());
        }
    }

    pub fn mark_rate_limited(key: &str) {
        let mut state = STATE.lock();
        roll_day(&mut state);
        let until = current_unix_secs() + RATE_LIMIT_COOLDOWN_SECS;
        if let Some(backoff) = state.temporary_backoffs.iter_mut().find(|b| b.key == key) {
            backoff.until = until;
        } else {
            state.temporary_backoffs.push(TemporaryBackoff {
                key: key.to_string(),
                until,
            });
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum LlmFailure {
    DailyQuota,
    RateLimited,
    Transient,
    Fatal,
}

pub(super) fn style_prompt_with_universal_directives(
    style_system_prompt: &str,
    universal_directives: &str,
) -> String {
    let directives = universal_directives.trim();
    if directives.is_empty() {
        return style_system_prompt.to_string();
    }

    format!(
        "{}\n\n# ユーザー共通指示\n{}",
        style_system_prompt.trim_end(),
        directives
    )
}

fn classify_llm_error(e: &LLMError) -> LlmFailure {
    match e {
        LLMError::InvalidResponse { status, body } if *status == 429 => {
            classify_rate_limit_body(body)
        }
        LLMError::InvalidResponse { status, .. } if *status >= 500 => LlmFailure::Transient,
        LLMError::InvalidResponse { status, body } if *status == 400 || *status == 404 => {
            if looks_like_model_unavailable(body) {
                LlmFailure::Transient
            } else {
                LlmFailure::Fatal
            }
        }
        _ => LlmFailure::Fatal,
    }
}

fn classify_rate_limit_body(body: &str) -> LlmFailure {
    let lower = body.to_ascii_lowercase();
    if lower.contains("tokens per day")
        || lower.contains("requests per day")
        || lower.contains("(tpd)")
        || lower.contains("(rpd)")
    {
        LlmFailure::DailyQuota
    } else {
        LlmFailure::RateLimited
    }
}

fn looks_like_model_unavailable(body: &str) -> bool {
    let lower = body.to_ascii_lowercase();
    lower.contains("model")
        && (lower.contains("decommissioned")
            || lower.contains("does not exist")
            || lower.contains("not found")
            || lower.contains("not available")
            || lower.contains("unavailable")
            || lower.contains("unsupported model")
            || lower.contains("no endpoint"))
}

fn build_model_chain(primary: &str, base_url: &str) -> Vec<String> {
    let mut chain = vec![primary.to_string()];
    if base_url.contains("groq.com") {
        for m in [
            "openai/gpt-oss-20b",
            "llama-3.3-70b-versatile",
            "meta-llama/llama-4-scout-17b-16e-instruct",
            "groq/compound-mini",
            "qwen/qwen3-32b",
        ] {
            if !chain.iter().any(|c| c == m) {
                chain.push(m.to_string());
            }
        }
    }
    chain
}

fn fallback_scope(active: &str, base_url: &str, api_key: &str) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    api_key.hash(&mut hasher);
    format!(
        "{}|{}|{:016x}",
        active,
        base_url.trim_end_matches('/'),
        hasher.finish()
    )
}

fn fallback_key(scope: &str, model: &str) -> String {
    format!("{scope}|{model}")
}

fn models_to_try<'a>(chain: &'a [String], scope: &str) -> Vec<&'a String> {
    let active: Vec<&String> = chain
        .iter()
        .filter(|m| !model_fallback::is_exhausted(&fallback_key(scope, m.as_str())))
        .collect();
    if active.is_empty() {
        chain.iter().collect()
    } else {
        active
    }
}

fn openai_fallback_setup() -> anyhow::Result<(String, String, String, Vec<String>)> {
    let active = CredentialsVault::get_active_llm();
    let api_key = CredentialsVault::get(CredentialAccount::ArkApiKey)?.unwrap_or_default();
    let model = CredentialsVault::get(CredentialAccount::ArkModelId)?
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "deepseek-v3-2".to_string());
    let endpoint = resolve_ark_endpoint(&api_key)?;
    let base_url = endpoint
        .trim_end_matches("/chat/completions")
        .trim_end_matches('/')
        .to_string();
    let chain = build_model_chain(&model, &base_url);
    Ok((active, api_key, base_url, chain))
}

fn build_openai_provider_for_model(
    active: &str,
    api_key: &str,
    base_url: &str,
    model: &str,
    llm_thinking_enabled: bool,
) -> OpenAICompatibleLLMProvider {
    let config = OpenAICompatibleConfig::new(
        active.to_string(),
        "OpenLess LLM",
        base_url.to_string(),
        api_key.to_string(),
        model.to_string(),
    )
    .with_thinking_enabled(llm_thinking_enabled);
    OpenAICompatibleLLMProvider::new(config)
}

/// 润色文本；失败时返回原文 + 失败原因，调用方据此弹错误胶囊 + 写历史 error_code。
/// 之前固定返回 String，调用方拿不到失败信号 → 用户感知"为什么风格设置没生效"。issue #57。
/// 流式润色的三态结果。让上层（dictation pipeline）能区分「已经流出去了」、
/// 「降级到一次性」和「真失败了走 raw 兜底」三种 case。
pub enum StreamingPolishOutcome {
    /// 流式润色成功，`String` 是已经一边流一边交给 `on_delta` 的全部文本（用于写
    /// history、做词条命中统计）。调用方不应再 `inserter.insert(&text)`，因为字符
    /// 已经通过键盘事件落到光标处。
    Streamed(String),
    /// 当前配置不支持流式：用户没开 streaming_insert / Gemini provider / Codex
    /// provider / Raw 模式 / 翻译模式 / 不是 macOS。调用方应回到现有的
    /// `polish_or_passthrough` 一次性路径，跟历史行为完全一致。
    UnsupportedFallback,
    /// 流式过程中失败（HTTP / 解析 / 空流等）。`String` 是失败原因，调用方应当
    /// 走 raw 兜底（同 `polish_or_passthrough` 失败分支的语义）。
    Failed(String),
}

/// 流式润色入口。在不支持流式的所有 case 都返回 `UnsupportedFallback`，让调用方
/// 透明降级。不修改任何持久化 / 焦点 / 光标状态。
///
/// `on_delta` 每收到一个 SSE chunk 就被调用一次（同步），调用方负责把 chunk 实际
/// 模拟键盘事件落到光标 —— 见 `coordinator/dictation.rs` 的流式分支。
/// `should_cancel` 用户取消时返回 true，立即 break SSE 读循环避免烧 quota。
pub async fn polish_or_passthrough_streaming<F, C>(
    raw: &RawTranscript,
    mode: PolishMode,
    hotwords: &[String],
    style_system_prompt: &str,
    working_languages: &[String],
    chinese_script_preference: ChineseScriptPreference,
    output_language_preference: OutputLanguagePreference,
    llm_thinking_enabled: bool,
    front_app: Option<&str>,
    prior_turns: &[(String, String)],
    on_delta: F,
    should_cancel: C,
) -> StreamingPolishOutcome
where
    F: Fn(&str) + Send + Sync,
    C: Fn() -> bool + Send + Sync,
{
    if mode == PolishMode::Raw && !raw_mode_uses_llm(style_system_prompt) {
        log::info!("[coord] streaming polish skipped: mode=Raw, fall back to one-shot");
        return StreamingPolishOutcome::UnsupportedFallback;
    }
    let active_llm = CredentialsVault::get_active_llm();
    if active_llm == "gemini" {
        log::info!(
            "[coord] streaming polish skipped: active LLM provider=gemini (v1 not implemented), fall back to one-shot"
        );
        return StreamingPolishOutcome::UnsupportedFallback;
    }
    if active_llm == CODEX_OAUTH_PROVIDER_ID {
        log::info!(
            "[coord] streaming polish skipped: provider does not support streaming (likely codex OAuth), fall back to one-shot"
        );
        return StreamingPolishOutcome::UnsupportedFallback;
    }
    let (active, api_key, base_url, chain) = match openai_fallback_setup() {
        Ok(setup) => setup,
        Err(e) => {
            log::error!("[coord] streaming polish: build provider failed: {e}");
            return StreamingPolishOutcome::Failed(e.to_string());
        }
    };
    log::info!(
        "[coord] streaming polish START: provider=openai-compatible mode={:?} raw_chars={} prior_turns={}",
        mode,
        raw.text.chars().count(),
        prior_turns.len()
    );
    let scope = fallback_scope(&active, &base_url, &api_key);
    let mut last_error: Option<LLMError> = None;
    for candidate in models_to_try(&chain, &scope) {
        let provider = build_openai_provider_for_model(
            &active,
            &api_key,
            &base_url,
            candidate,
            llm_thinking_enabled,
        );
        let emitted_any_delta = AtomicBool::new(false);
        let result = provider
            .polish_streaming(
                &raw.text,
                mode,
                hotwords,
                style_system_prompt,
                working_languages,
                chinese_script_preference,
                output_language_preference,
                front_app,
                prior_turns,
                |delta| {
                    emitted_any_delta.store(true, Ordering::Relaxed);
                    on_delta(delta);
                },
                &should_cancel,
            )
            .await;
        match result {
            Ok(text) => {
                if candidate != &chain[0] {
                    log::warn!(
                        "[coord] streaming polish: メインモデル '{}' が使えないため予備モデル '{}' で整形しました",
                        chain[0],
                        candidate
                    );
                }
                log::info!(
                    "[coord] streaming polish OK: final_chars={}",
                    text.chars().count()
                );
                return StreamingPolishOutcome::Streamed(text);
            }
            Err(e) if emitted_any_delta.load(Ordering::Relaxed) => {
                let reason = e.to_string();
                log::error!(
                    "[coord] streaming polish FAILED after deltas; not retrying to avoid duplicate insertion: {reason}"
                );
                return StreamingPolishOutcome::Failed(reason);
            }
            Err(e) => match classify_llm_error(&e) {
                LlmFailure::DailyQuota => {
                    model_fallback::mark_daily_exhausted(&fallback_key(&scope, candidate));
                    last_error = Some(e);
                    continue;
                }
                LlmFailure::RateLimited => {
                    model_fallback::mark_rate_limited(&fallback_key(&scope, candidate));
                    last_error = Some(e);
                    continue;
                }
                LlmFailure::Transient => {
                    last_error = Some(e);
                    continue;
                }
                LlmFailure::Fatal => {
                    let reason = e.to_string();
                    log::error!("[coord] streaming polish FAILED: {reason}");
                    return StreamingPolishOutcome::Failed(reason);
                }
            },
        }
    }
    let reason = last_error
        .unwrap_or_else(|| LLMError::Network("no LLM model candidates available".into()))
        .to_string();
    log::error!("[coord] streaming polish FAILED: {reason}");
    StreamingPolishOutcome::Failed(reason)
}

pub(super) async fn polish_or_passthrough(
    raw: &RawTranscript,
    mode: PolishMode,
    hotwords: &[String],
    style_system_prompt: &str,
    working_languages: &[String],
    chinese_script_preference: ChineseScriptPreference,
    output_language_preference: OutputLanguagePreference,
    llm_thinking_enabled: bool,
    front_app: Option<&str>,
    prior_turns: &[(String, String)],
) -> (String, Option<String>) {
    if mode == PolishMode::Raw && !raw_mode_uses_llm(style_system_prompt) {
        return (raw.text.clone(), None);
    }
    match polish_text(
        &raw.text,
        mode,
        hotwords,
        style_system_prompt,
        working_languages,
        chinese_script_preference,
        output_language_preference,
        llm_thinking_enabled,
        front_app,
        prior_turns,
    )
    .await
    {
        Ok(s) => (s, None),
        Err(e) => {
            let reason = e.to_string();
            log::error!("[coord] polish failed, falling back to raw: {reason}");
            (raw.text.clone(), Some(reason))
        }
    }
}

pub(super) async fn polish_text(
    raw: &str,
    mode: PolishMode,
    hotwords: &[String],
    style_system_prompt: &str,
    working_languages: &[String],
    chinese_script_preference: ChineseScriptPreference,
    output_language_preference: OutputLanguagePreference,
    llm_thinking_enabled: bool,
    front_app: Option<&str>,
    prior_turns: &[(String, String)],
) -> anyhow::Result<String> {
    // 谷歌 Gemini 分支：所有 LLM provider 共用 ark.* 凭据槽，唯独 Gemini 走原生
    // generateContent / 自带 thinkingConfig 控制；其余 provider 走 OpenAI
    // 兼容协议，并在该路径里按 provider/channel 下发对应的思考开关。
    let active_llm = CredentialsVault::get_active_llm();
    if active_llm == "gemini" {
        let (api_key, model, base_url) = read_gemini_credentials()?;
        let provider = GeminiProvider::new(
            GeminiConfig::new(api_key, model, base_url).with_thinking_enabled(llm_thinking_enabled),
        );
        return Ok(provider
            .polish(
                raw,
                mode,
                hotwords,
                style_system_prompt,
                working_languages,
                chinese_script_preference,
                output_language_preference,
                front_app,
                prior_turns,
            )
            .await?);
    }

    if CredentialsVault::get_active_llm() == CODEX_OAUTH_PROVIDER_ID {
        let provider = build_active_llm_provider(llm_thinking_enabled)?;
        return Ok(provider
            .polish(
                raw,
                mode,
                hotwords,
                style_system_prompt,
                working_languages,
                chinese_script_preference,
                output_language_preference,
                front_app,
                prior_turns,
            )
            .await?);
    }

    let (active, api_key, base_url, chain) = openai_fallback_setup()?;
    let scope = fallback_scope(&active, &base_url, &api_key);
    let mut last_error: Option<LLMError> = None;
    for candidate in models_to_try(&chain, &scope) {
        let provider = build_openai_provider_for_model(
            &active,
            &api_key,
            &base_url,
            candidate,
            llm_thinking_enabled,
        );
        match provider
            .polish(
                raw,
                mode,
                hotwords,
                style_system_prompt,
                working_languages,
                chinese_script_preference,
                output_language_preference,
                front_app,
                prior_turns,
            )
            .await
        {
            Ok(s) => {
                if candidate != &chain[0] {
                    log::warn!(
                        "[coord] polish: メインモデル '{}' が使えないため予備モデル '{}' で整形しました",
                        chain[0],
                        candidate
                    );
                }
                return Ok(s);
            }
            Err(e) => match classify_llm_error(&e) {
                LlmFailure::DailyQuota => {
                    model_fallback::mark_daily_exhausted(&fallback_key(&scope, candidate));
                    last_error = Some(e);
                    continue;
                }
                LlmFailure::RateLimited => {
                    model_fallback::mark_rate_limited(&fallback_key(&scope, candidate));
                    last_error = Some(e);
                    continue;
                }
                LlmFailure::Transient => {
                    last_error = Some(e);
                    continue;
                }
                LlmFailure::Fatal => return Err(e.into()),
            },
        }
    }
    Err(last_error
        .unwrap_or_else(|| LLMError::Network("no LLM model candidates available".into()))
        .into())
}

/// 专用翻译（仅翻译、不润色、单轮）。现作为"润色+翻译"合成调用解析失败时的兜底——
/// 模型没按两段格式输出时，退回这里拿一段干净译文，而不是把畸形输出当译文插入。
pub(super) async fn translate_text(
    raw: &str,
    target_language: &str,
    working_languages: &[String],
    chinese_script_preference: ChineseScriptPreference,
    output_language_preference: OutputLanguagePreference,
    llm_thinking_enabled: bool,
    front_app: Option<&str>,
) -> anyhow::Result<String> {
    // 见 polish_text 顶部注释——同样的 Gemini / OpenAI-compatible 路由逻辑。
    let active_llm = CredentialsVault::get_active_llm();
    if active_llm == "gemini" {
        let (api_key, model, base_url) = read_gemini_credentials()?;
        let provider = GeminiProvider::new(
            GeminiConfig::new(api_key, model, base_url).with_thinking_enabled(llm_thinking_enabled),
        );
        return Ok(provider
            .translate_to(
                raw,
                target_language,
                working_languages,
                chinese_script_preference,
                output_language_preference,
                front_app,
            )
            .await?);
    }

    if CredentialsVault::get_active_llm() == CODEX_OAUTH_PROVIDER_ID {
        let provider = build_active_llm_provider(llm_thinking_enabled)?;
        return Ok(provider
            .translate_to(
                raw,
                target_language,
                working_languages,
                chinese_script_preference,
                output_language_preference,
                front_app,
            )
            .await?);
    }

    let (active, api_key, base_url, chain) = openai_fallback_setup()?;
    let scope = fallback_scope(&active, &base_url, &api_key);
    let mut last_error: Option<LLMError> = None;
    for candidate in models_to_try(&chain, &scope) {
        let provider = build_openai_provider_for_model(
            &active,
            &api_key,
            &base_url,
            candidate,
            llm_thinking_enabled,
        );
        match provider
            .translate_to(
                raw,
                target_language,
                working_languages,
                chinese_script_preference,
                output_language_preference,
                front_app,
            )
            .await
        {
            Ok(s) => {
                if candidate != &chain[0] {
                    log::warn!(
                        "[coord] translate: メインモデル '{}' が使えないため予備モデル '{}' で翻訳しました",
                        chain[0],
                        candidate
                    );
                }
                return Ok(s);
            }
            Err(e) => match classify_llm_error(&e) {
                LlmFailure::DailyQuota => {
                    model_fallback::mark_daily_exhausted(&fallback_key(&scope, candidate));
                    last_error = Some(e);
                    continue;
                }
                LlmFailure::RateLimited => {
                    model_fallback::mark_rate_limited(&fallback_key(&scope, candidate));
                    last_error = Some(e);
                    continue;
                }
                LlmFailure::Transient => {
                    last_error = Some(e);
                    continue;
                }
                LlmFailure::Fatal => return Err(e.into()),
            },
        }
    }
    Err(last_error
        .unwrap_or_else(|| LLMError::Network("no LLM model candidates available".into()))
        .into())
}

/// "润色+翻译"单次调用的两段哨兵。模型按 `SRC\n源文\nTGT\n译文` 输出，解析器据此切分。
/// 这两个串必须与 build_polish_translate_system_prompt 写给模型的完全一致。
pub(super) const POLISH_TRANSLATE_SRC_MARKER: &str = "[[OPENLESS_POLISHED_SOURCE]]";
pub(super) const POLISH_TRANSLATE_TGT_MARKER: &str = "[[OPENLESS_TRANSLATION]]";

/// 合成"先润色源文、再翻译"的系统提示词：在原翻译 prompt 之上追加"额外输出润色后源文"
/// 与严格两段格式（覆盖原 prompt 末尾的"只输出译文"）。译文仍是要插入用户光标的主产物，
/// 故完整保留原翻译规则；润色后的源文只作对话上下文用，轻量清理即可。
pub(super) fn build_polish_translate_system_prompt(target_language: &str) -> String {
    let base = crate::polish::prompts::translate_system_prompt(target_language);
    format!(
        "{base}\n\n\
         # 额外输出：润色后的源文（仅用于对话上下文，不展示给用户）\n\
         在译文之前，先把上面的原始转写**按它本来的语言**润色一遍：去掉口癖（嗯 / 那个 / um）、\
         补必要标点、纠正明显的识别错误，但**不翻译、不改写风格、不增删意思**。\n\n\
         # 输出格式（覆盖上面\u{201C}只输出译文\u{201D}的说明，严格遵守）\n\
         严格按下面两段输出，两个标记必须原样出现、各占一行，标记之外不要有任何多余文字：\n\
         {src}\n\
         （这里放润色后的源文，保持原语言）\n\
         {tgt}\n\
         （这里放翻译成\u{300C}{lang}\u{300D}的译文）",
        base = base,
        src = POLISH_TRANSLATE_SRC_MARKER,
        tgt = POLISH_TRANSLATE_TGT_MARKER,
        lang = target_language,
    )
}

/// 解析"润色+翻译"单次调用输出 → Some((润色后源文, 译文))。
/// 找到译文标记且译文非空 → Some((源文, 译文))：源文标记缺失 / 源文段为空时源文为 None，
/// 译文取标记之后的干净正文。**没有译文标记、或译文段为空（模型截断 / 只吐了标记）→ None**，
/// 表示没拿到可信译文，交由调用方退回专用翻译——避免把空串当"成功译文"插进光标而丢字。
pub(super) fn split_polish_translate_output(raw: &str) -> Option<(Option<String>, String)> {
    let tgt_idx = raw.find(POLISH_TRANSLATE_TGT_MARKER)?;
    let translation = raw[tgt_idx + POLISH_TRANSLATE_TGT_MARKER.len()..]
        .trim()
        .to_string();
    if translation.is_empty() {
        return None;
    }
    let before_tgt = &raw[..tgt_idx];
    let source = before_tgt
        .find(POLISH_TRANSLATE_SRC_MARKER)
        .map(|i| {
            before_tgt[i + POLISH_TRANSLATE_SRC_MARKER.len()..]
                .trim()
                .to_string()
        })
        .filter(|s| !s.is_empty());
    Some((source, translation))
}

/// 翻译路径——单次 LLM 调用同时润色源文 + 翻译。和 polish 一样失败时返回原文 + 失败原因，
/// 避免"不丢字"约定被违反（CLAUDE.md）。返回 (要插入的译文, 润色后源文供上下文用, 失败原因)。
#[allow(clippy::too_many_arguments)]
pub(super) async fn polish_and_translate_or_passthrough(
    raw: &RawTranscript,
    target_language: &str,
    mode: PolishMode,
    hotwords: &[String],
    working_languages: &[String],
    chinese_script_preference: ChineseScriptPreference,
    output_language_preference: OutputLanguagePreference,
    llm_thinking_enabled: bool,
    front_app: Option<&str>,
    prior_turns: &[(String, String)],
) -> (String, Option<String>, Option<String>) {
    let system_prompt = build_polish_translate_system_prompt(target_language);
    match polish_text(
        &raw.text,
        mode,
        hotwords,
        &system_prompt,
        working_languages,
        chinese_script_preference,
        output_language_preference,
        llm_thinking_enabled,
        front_app,
        prior_turns,
    )
    .await
    {
        Ok(out) => match split_polish_translate_output(&out) {
            Some((source, translation)) => (translation, source, None),
            None => {
                // 模型没按两段格式输出：退回专用翻译拿一段干净译文，避免把畸形输出插进光标。
                // 此时无可信源文，这条翻译历史不参与后续普通润色上下文。
                log::warn!(
                    "[coord] polish+translate output missing markers; falling back to plain translate"
                );
                match translate_text(
                    &raw.text,
                    target_language,
                    working_languages,
                    chinese_script_preference,
                    output_language_preference,
                    llm_thinking_enabled,
                    front_app,
                )
                .await
                {
                    Ok(translation) => (translation, None, None),
                    Err(e) => {
                        let reason = e.to_string();
                        log::error!("[coord] fallback translate failed, using raw: {reason}");
                        (raw.text.clone(), None, Some(reason))
                    }
                }
            }
        },
        Err(e) => {
            let reason = e.to_string();
            log::error!("[coord] polish+translate failed, falling back to raw: {reason}");
            (raw.text.clone(), None, Some(reason))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groq_model_chain_tries_free_quality_fallbacks_before_qwen() {
        assert_eq!(
            build_model_chain("openai/gpt-oss-120b", "https://api.groq.com/openai/v1"),
            vec![
                "openai/gpt-oss-120b",
                "openai/gpt-oss-20b",
                "llama-3.3-70b-versatile",
                "meta-llama/llama-4-scout-17b-16e-instruct",
                "groq/compound-mini",
                "qwen/qwen3-32b",
            ]
        );
    }

    #[test]
    fn groq_tpd_429_is_daily_quota() {
        let err = LLMError::InvalidResponse {
            status: 429,
            body: "Rate limit reached for model `openai/gpt-oss-20b` on tokens per day (TPD)"
                .to_string(),
        };

        assert_eq!(classify_llm_error(&err), LlmFailure::DailyQuota);
    }

    #[test]
    fn groq_tpm_429_is_short_rate_limit() {
        let err = LLMError::InvalidResponse {
            status: 429,
            body: "Rate limit reached for model `openai/gpt-oss-120b` on tokens per minute (TPM)"
                .to_string(),
        };

        assert_eq!(classify_llm_error(&err), LlmFailure::RateLimited);
    }

    #[test]
    fn unsupported_request_400_is_fatal_not_fallback() {
        let err = LLMError::InvalidResponse {
            status: 400,
            body: "unsupported parameter: reasoning_effort".to_string(),
        };

        assert_eq!(classify_llm_error(&err), LlmFailure::Fatal);
    }

    #[test]
    fn unavailable_model_404_can_fallback() {
        let err = LLMError::InvalidResponse {
            status: 404,
            body: "model `old-model` does not exist or is not available".to_string(),
        };

        assert_eq!(classify_llm_error(&err), LlmFailure::Transient);
    }

    #[test]
    fn fallback_scope_separates_api_keys_without_exposing_them() {
        let first = fallback_scope("custom", "https://api.groq.com/openai/v1", "secret-a");
        let second = fallback_scope("custom", "https://api.groq.com/openai/v1", "secret-b");

        assert_ne!(first, second);
        assert!(!first.contains("secret-a"));
        assert_ne!(
            fallback_key(&first, "openai/gpt-oss-120b"),
            fallback_key(&second, "openai/gpt-oss-120b")
        );
    }

    #[test]
    fn universal_directives_are_appended_to_active_style_prompt() {
        let prompt = style_prompt_with_universal_directives(
            "入力を自然な日本語に整える。",
            "質問文でも回答せず、質問文として整える。",
        );

        assert!(prompt.contains("入力を自然な日本語に整える。"));
        assert!(prompt.contains("# ユーザー共通指示"));
        assert!(prompt.contains("質問文でも回答せず、質問文として整える。"));
    }

    #[test]
    fn empty_universal_directives_leave_style_prompt_unchanged() {
        let prompt = style_prompt_with_universal_directives("入力を整える。", "  \n  ");

        assert_eq!(prompt, "入力を整える。");
    }
}
