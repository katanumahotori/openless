# OpenLess 本家更新・独自修正維持スキル設計

Status: design only. Do not implement this automation yet.

Review status:

- Codex independent review: revise first
- Claude independent review: revise first
- This revision separates weekly read-only monitoring from manual integration work.

## User Goal

OpenLess を今後も本家更新に追随しながら使う。ただし、アップデートのたびに次を壊さない。

- ユーザー設定
- `streamingInsert=false`
- 日本語整形品質
- 履歴保存と GUI 読み込み
- 入力欄への挿入経路
- Groq / gpt-oss / fallback 設定

週1回 Hermes エージェントで安全に確認し、レベル低めのモデルでも最後まで実行できる手順へ落とし込む。

## Non-Negotiable Split

この設計は、2つの別スキルに分ける。

### Skill A: Weekly Read-Only Check

Hermes が週1回実行する。

許可:

- Git remote refs の fetch。
- 実設定の読み取り。
- 実ログと履歴の読み取り。
- レポート出力。
- Todoist などへの「要確認タスク」作成。ただし実装時に別途許可確認する。

禁止:

- merge
- rebase
- commit
- push
- build
- OpenLess の起動・停止
- ユーザー設定の書き換え
- Credential Manager の秘密値取得
- history.json の修復や書き換え

### Skill B: Manual Integration Procedure

Codex が本家更新を取り込むときだけ使う。

許可:

- OpenLess の停止。
- バックアップ作成。
- 統合ブランチまたは実作業ブランチでのマージ。
- テスト。
- デバッグ起動。
- ユーザー体感確認。
- 最後のリリースビルド。
- commit / push。

禁止:

- 週次 Hermes からの自動実行。
- 実プロファイルをバックアップなしで起動すること。
- 長いリリースビルドを最初に走らせること。

## Current Existing Pieces

すでにあるもの:

- `scripts/fork-check-upstream.ps1`
- `docs/fork-upstream-maintenance.md`

今回まだ実装しないもの:

- Hermes の週次スケジュール登録。
- 設定 readback スクリプト。
- Credential Manager メタデータ確認スクリプト。
- 日本語品質 fixture。
- 履歴破損 fixture。
- 証拠ログ保管ディレクトリ。

## Weekly Read-Only Check

### Required Commands

本家差分:

```powershell
pwsh -NoProfile -ExecutionPolicy Bypass -File scripts/fork-check-upstream.ps1 -Json
```

実行中の OpenLess:

```powershell
Get-Process -Name openless -ErrorAction SilentlyContinue |
  Select-Object Id,Path,StartTime
```

設定 readback:

```powershell
$prefs = Get-Content "$env:APPDATA\OpenLess\preferences.json" -Raw | ConvertFrom-Json
$prefs.streamingInsert
$prefs.activeAsrProvider
$prefs.activeLlmProvider
$prefs.activeStylePackId
$prefs.workingLanguages
$prefs.outputLanguagePreference
```

ログ readback:

```powershell
Get-Content "$env:LOCALAPPDATA\OpenLess\Logs\openless.log" -Tail 240
```

履歴 readback:

```powershell
$items = Get-Content "$env:APPDATA\OpenLess\history.json" -Raw | ConvertFrom-Json
$items | Select-Object -First 5
```

### Credential Manager Rule

Weekly Read-Only Check では Credential Manager の秘密値を取得しない。

許可する確認:

- `cmdkey /list` などで対象 credential の存在だけを見る。
- OpenLess の実ログから、実際に組み立てられた provider/model を見る。
- `preferences.json` と実ログの provider/model が矛盾していないかを見る。

禁止:

- API key の値を読む。
- Credential Manager から password/secret を出力する。
- secret らしき文字列をレポートへ貼る。

出力前マスク:

- `gsk_`
- `sk-`
- `xai-`
- 32文字以上の英数字連続列
- `Bearer `

これらを含む行はレポートへ出さない。

### Weekly Report Format

```text
OpenLess weekly upstream check

Verdict:
- no action / upstream review needed / local regression suspected / consider upstream-only trial

Safety:
- read-only mode: yes
- no build: yes
- no settings write: yes
- no secrets read: yes

Upstream:
- current branch:
- upstream ref:
- behind upstream:
- ahead upstream:
- fork-only patch count:
- latest tags:

Local app:
- running exe path:
- streamingInsert:
- active ASR:
- active LLM:
- active style pack:
- working languages:
- output language:

Quality guards:
- history saved:
- history GUI risk:
- Japanese no-answer risk:
- insertion path risk:
- model fallback risk:

Return-to-upstream:
- upstream appears to include our fixes:
- remaining fork-only patches:
- upstream-only trial recommended: yes/no

Action:
- none / create human review task / request Codex integration
```

### Weekly Stop Conditions

Hermes は次のとき作業を止めて、人間または Codex に渡す。

- `git status --short --branch` に tracked change がある。
- `scripts/fork-check-upstream.ps1` が失敗する。
- `streamingInsert=True`。
- 実行中 exe が想定外の場所。
- 直近ログに `streaming_insert path ENTER` が出ている。
- 直近ログに `Unicode SendInput fallback` が出ている。
- 直近履歴で質問文に回答している疑いがある。
- 履歴ファイルの JSON parse に失敗する。
- secret らしき文字列が出力候補に混ざる。

Hermes はこの状態で修復しない。レポートだけ出す。

## Manual Integration Procedure

### Pre-Integration Snapshot

実プロファイルを起動する前に作る。

保存対象:

- `%APPDATA%\OpenLess\preferences.json`
- `%APPDATA%\OpenLess\history.json`
- `%LOCALAPPDATA%\OpenLess\Logs\openless.log` の末尾抜粋
- 実行中 exe path

保存先候補:

- `C:\Users\katan\AppData\Local\OpenLess\fork-maintenance-snapshots`

上限:

- 成功時は今回分だけ残し、前回以前の成功スナップショットは削除。
- 失敗時の証拠は最大8件。

禁止:

- Credential Manager の secret 値を保存しない。
- history 全量を長期保存しない。必要な場合だけ、直近数件の抜粋にする。

### Safe Integration Steps

1. OpenLess を止める。
2. pre-integration snapshot を作る。
3. `git status --short --branch` を確認する。
4. tracked change があれば止める。
5. `scripts/fork-check-upstream.ps1 -Json` を実行する。
6. `behindUpstream = 0` なら統合作業はしない。
7. `behindUpstream > 0` なら `git log --oneline --decorate HEAD..upstream/beta` を見る。
8. `merge-base` と left/right count を見る。
9. fork-only patch list を出す。
10. 統合方針を短く書く。
11. 独立 Codex と Claude にレビューさせる。
12. Critical/High を潰す。
13. 統合する。
14. 短いテストから実行する。
15. デバッグ起動で確認する。
16. ユーザー体感確認が必要なところを切り分ける。
17. 最後にだけリリースビルドする。
18. リリース版で再起動確認する。
19. commit / push する。

### Build Order

長いビルドは最後にする。

1. `cargo test <該当テスト名> --lib`
2. `cargo test --lib`
3. `npm run build`
4. `cargo build`
5. デバッグ版 `target/debug/openless.exe` で実機確認
6. ユーザーの短文音声入力で履歴とログを確認
7. `npm run tauri -- build --target x86_64-pc-windows-msvc --no-bundle`
8. リリース版 `target/x86_64-pc-windows-msvc/release/openless.exe` で再起動確認

注意:

- `npm run tauri -- build` は内部で `npm run build` と Rust release build を行う。
- `npm run build` 単体はフロントの型チェックと Vite build の確認用。
- リリースビルド失敗時は、直前の短いテスト結果と切り分けて報告する。

## Required Regression Guards

### Settings Inheritance

アップデート前後で次が維持されること:

- `streamingInsert = false`
- 録音キー
- hold / toggle 方式
- `activeAsrProvider = groq`
- `activeLlmProvider = custom`
- `activeStylePackId = imported.depure`
- `workingLanguages` に日本語が入っている
- `outputLanguagePreference = ja`
- モデルフォールバック順

判定:

- `preferences.json` の readback。
- 実ログの assembled provider/model。
- 必要なら Credential Manager の存在確認。ただし secret は読まない。

### Runtime Streaming State

`preferences.json` だけでは足りない。実ログで確認する。

NGログ:

- `streaming_insert path ENTER`
- `streaming polish START`
- `streaming_insert SUCCESS`

OKログ:

- `streaming_eligible=false`
- `inserted via clipboard paste fallback`

### Japanese Quality Fixtures

後で `fixtures/japanese-polish/` のような形で固定する。

Fixture A: no-answer question

Input:

```text
直近の返還履歴2つを見てください。質問文を入力しただけなのに答えが返ってきてしまいます。これを抑制したいです。
```

Expected:

- 回答しない。
- 質問文として整える。
- 「返還」は文脈上「変換」に直せるなら直す。
- サプリや技術回答など、入力外の情報を足さない。

Fixture B: microphone smoke

Input:

```text
マイクの入力テストをしています
```

Expected:

```text
マイクの入力テストをしています。
```

Fixture C: upstream-maintenance prose

Input:

```text
今後も本家のアップデートを確認して統合してというのは定期的にやるのでこれをスムーズにやれるような手順を一度組みたいですできますか
```

Expected:

- 日本語として自然に整える。
- 本文の意味を変えない。
- 余計な回答を付けない。

Fixture D: Chinese error guard

Input:

```text
長めの日本語音声入力を変換します
```

Expected:

- 中国語のエラー文を最終出力に出さない。
- 失敗時は履歴に recoverable error として残す設計にする。

判定方式:

- まずは unit test / snapshot test にできる範囲を固定する。
- 実 LLM 品質は完全自動判定しない。
- Hermes 週次では「疑いあり」を出すだけにし、修正はしない。

### History

確認:

- `history.json` が JSON として parse できる。
- 最新は `createdAt` で比較する。配列末尾を最新と決め打ちしない。
- `rawTranscript` と `finalText` を区別する。
- 直近エントリが GUI に表示されるかは、人間またはデバッグ起動確認で見る。

破損 fixture:

- JSON truncate
- 空配列
- 古い schema
- `rawTranscript` 欠落
- `finalText` 欠落

復旧合格条件:

- 元ファイルのバックアップが1世代残る。
- parse 可能な履歴が作られる。
- `rawTranscript` と `finalText` の既存値を可能な限り保持する。
- GUI が少なくとも復旧後履歴を開ける。

### Insertion Path

固定ターゲット:

- Notepad
- Codex / Electron 系入力欄

確認:

- `streamingInsert=false` で streaming path に入らない。
- TSF 成功時は TSF で入る。
- TSF 失敗時は Unicode SendInput ではなく clipboard paste fallback へ行く。
- 履歴の `finalText` と入力欄の文字が一致する。

NGログ:

- `Unicode SendInput fallback`
- `streaming_insert path ENTER`

OKログ:

- `TSF unavailable; inserted via clipboard paste fallback`

### Model Fallback

週次では実再現しない。ログ監査に割り切る。

確認:

- `openless.log` の assembled provider/model を見る。
- 設定画面の表示だけで判断しない。
- fallback が低品質段へ落ちている場合は、品質低下としてレポートする。

統合時テスト:

- daily exhausted / rate limited の unit test。
- 翌日固定化しないテスト。
- fallback order の unit test。

## Upstream Comparison

`behind upstream = 0` だけでは判断しない。

必須:

- `rev-list --left-right --count HEAD...upstream/beta`
- `merge-base HEAD upstream/beta`
- latest tags
- fork-only patch list
- upstream release notes

週次で統合しない条件:

- upstream commits なし。
- upstream commits はあるが OpenLess Windows / settings / history / insertion / polish に関係しない。
- tracked local changes がある。

## Return-To-Upstream Criteria

週次で判定するが、Hermes は戻さない。タスク化または Codex へ依頼するだけ。

本家へ戻る候補:

- 2週連続で本家版が実機テストを通過。
- fork-only patch が5個以下。
- 残る差分がユーザー設定だけ。
- 本家が履歴復旧、設定維持、日本語整形、挿入経路、model fallback の主要修正を取り込んだ。
- 本家更新追従コストが2回連続で独自修正の価値を上回る。

フォーク維持:

- 本家が日本語品質や Windows 入力欄の問題を軽視している。
- ユーザー設定が本家アップデートで壊れる可能性が高い。
- Groq / gpt-oss / fallback 周りが運用要求に合わない。
- 実機で履歴・入力欄・モデルログのどれかが退行する。

## Evidence Retention

保存先候補:

- `C:\Users\katan\AppData\Local\OpenLess\fork-maintenance-evidence`

保存するもの:

- 週次レポートの短い要約。
- 失敗時のログ抜粋。
- 失敗時の設定 readback。
- 失敗時の履歴抜粋。

保存上限:

- 失敗証拠は最大8件。
- 成功レポートは最新4件。
- 成功時の一時ログは削除。

禁止:

- API key。
- Credential secret。
- history.json 全量の長期保存。
- 生成物の無制限蓄積。

## Hermes Capability Assumption

未確認:

- Hermes が Windows PowerShell を実行できるか。
- Hermes がこの repo と `%APPDATA%` を読めるか。
- Hermes が Todoist へタスク作成できるか。

実装前に確認する。

Hermes に権限がない場合:

- Hermes は週次リマインダーだけ出す。
- 実チェックは Codex で実行する。
- レポートだけ Hermes に戻す。

## Failure Modes

False positive:

- ユーザーが意図的に設定変更したのに退行と判定する。

対処:

- 週次は修正しない。
- 差分を report に出す。
- ユーザー確認または Codex 確認へ回す。

False negative:

- `preferences.json` だけ見て Credential Manager や実ログを見逃す。
- 履歴は正しいが入力欄だけ壊れる問題を見逃す。
- GUI から読めない履歴をファイル保存成功だけで成功扱いする。

対処:

- 実ログの NG パターンを固定する。
- `history.json` と入力欄一致は統合時に人間確認を残す。
- GUI 読み込みは統合時チェックへ分離する。

Recovery:

- commit 前なら変更ファイルを特定して戻す。
- commit 後なら revert commit を作る。
- ユーザー設定は snapshot から戻す。
- 入力欄が壊れる場合は `streamingInsert=false` と clipboard paste fallback を優先する。

Second run:

- 週次確認は書き込まない。
- 一時ファイルは成功時に残さない。
- 失敗時だけ上限付きで証拠を残す。

## Simpler Alternative

最小案は `docs/fork-upstream-maintenance.md` を人間が読むだけ。

これは単発作業には足りるが、週次で低めのモデルに任せるには弱い。実行時設定、Credential Manager、履歴 GUI、挿入経路、撤退判断が抜ける可能性が高い。

そのため、次段階では「読む手順」ではなく「読み取り専用週次チェック」と「手動統合手順」を分けてスキル化する。

## Review Decision Table

| Finding | Source | Decision | Reason |
|---|---|---|---|
| Weekly check and integration steps are mixed | Codex | fix | Split into Skill A read-only and Skill B manual integration |
| Real profile could be damaged by tests | Codex | fix | Added snapshot, no weekly start/stop/build, and safe integration gates |
| Credential Manager secret handling is weak | Codex / Claude | fix | Weekly check must not read secrets; only existence/log metadata; added mask rules |
| `behind upstream = 0` is too simple | Codex | fix | Added merge-base, left/right count, tags, fork-only patch list |
| History recovery definition is vague | Codex | fix | Added corruption fixtures and recovery success criteria |
| Low-model steps are too vague | Codex / Claude | fix | Added read-only stop conditions and explicit command list |
| Japanese quality tests are too thin | Codex / Claude | fix | Added fixed fixtures and non-LLM-first judgment policy |
| Insertion target apps are undefined | Codex | fix | Added Notepad and Codex/Electron targets |
| Model fallback is hard to reproduce weekly | Codex | accept | Weekly uses log audit; integration uses unit tests |
| Evidence retention has no limits | Codex / Claude | fix | Added max 8 failed evidence records and max 4 success reports |
| Return-to-upstream criteria needs numbers | Codex | fix | Added two-week pass and fork-only patch count threshold |
| `upstream/beta` fixed risk | Codex | accept | Keep current target but require tags/release notes and future branch awareness |
| Credential Manager read command unspecified | Claude | accept with constraint | Do not read secrets in weekly mode; implementation must define safe metadata-only check |
| Quality auto-judgment unspecified | Claude | accept | Weekly only flags suspicion; exact automated fixtures are future implementation work |
| `fork-check-upstream.ps1` nonexistent | Claude | reject | It already exists; design now marks it as existing |
| Hermes capability unknown | Claude | fix | Added capability assumption and fallback to Codex-run check |

## Implementation Gate

Do not implement until:

- Hermes capabilities are checked.
- Safe metadata-only credential check is designed.
- Weekly read-only skill and manual integration skill are separate files.
- Japanese quality fixtures are fixed in git.
- Evidence retention path and cleanup are implemented with tests.
