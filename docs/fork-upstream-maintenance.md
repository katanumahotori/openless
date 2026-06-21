# OpenLess 本家アップデート確認手順

このフォークは、ユーザーの実運用に合わせた独自修正を持っています。本家の更新は取り込みますが、勝手に自動マージしません。まず差分を見て、独自修正を守れるか確認します。

## まず確認する

```powershell
pwsh -NoProfile -ExecutionPolicy Bypass -File scripts/fork-check-upstream.ps1
```

このスクリプトが確認専用の正本です。作業ファイルや OpenLess の設定は変更せず、マージも push もしません。ただし、最新状況を見るために `git fetch` で Git のリモート参照だけ更新します。

見るポイント:

- `behind 0` なら、本家側に未取り込みの更新はありません。
- `behind 1` 以上なら、本家に新しい変更があります。
- `[warn] Tracked working-tree changes exist` が出たら、統合前に今の変更を commit するか、内容を確認します。
- 現在ブランチが `backup/all-fork-changes` でない場合は、誤った場所で作業している可能性があります。
- `upstream/beta` が見つからない場合は、本家の既定ブランチが変わった可能性があります。その時は `-UpstreamBranch main` のように比較先を変えます。

## 統合前に守る独自修正

本家更新を取り込むとき、少なくとも次は壊してはいけません。

- 履歴の読み込み互換性と、壊れた履歴ファイルからの復旧。
- Groq のモデル優先順と、一日上限後のフォールバック状態が翌日に固定化されない処理。
- `gpt-oss-120b`、Llama、Qwen などのモデル設定と、実行時ログで実モデルを確認できること。
- ユーザー設定、ホットキー、録音方式がアップデートで勝手に変わらないこと。
- 整形プロンプトが質問に答えず、入力文だけを整えること。
- Windows の実行ファイル、再起動用ショートカット、パッケージ作成手順。
- `streamingInsert` は既定 OFF。履歴が正しいのに入力欄だけ壊れる経路を再発させないこと。

## 統合する時の流れ

1. OpenLess を止める。音声入力中に作業しない。
2. `git status --short --branch` で未反映の変更を確認する。
3. `check-upstream.ps1` を実行して、本家差分の有無を見る。
4. 本家差分がある場合は、`git log --oneline --decorate HEAD..upstream/beta` で内容を確認する。
5. 本家の更新を取り込む。自動マージ後は、独自修正のファイルを重点的に見る。
6. テストする。

```powershell
cd openless-all/app
cargo test --lib
npm run build
```

7. Windows 実行ファイルを作る。

```powershell
cd openless-all/app
npm run tauri -- build --target x86_64-pc-windows-msvc --no-bundle
```

8. 作った `openless.exe` を起動し、実設定を確認する。

```powershell
$prefs = Get-Content "$env:APPDATA\OpenLess\preferences.json" -Raw | ConvertFrom-Json
$prefs.streamingInsert
$prefs.activeAsrProvider
$prefs.activeLlmProvider
$prefs.activeStylePackId
```

9. 実ログで、実際に使われたモデルと履歴保存を確認する。

```powershell
Get-Content "$env:LOCALAPPDATA\OpenLess\Logs\openless.log" -Tail 120
```

10. 問題がなければ commit して push する。

push 先は `origin` だけです。本家の `upstream` には push しません。

## 再発時の切り分け

履歴の `finalText` は正しいのに、入力欄の文章だけ壊れている場合は、モデル品質より先に `streamingInsert` を疑います。

```powershell
$prefs = Get-Content "$env:APPDATA\OpenLess\preferences.json" -Raw | ConvertFrom-Json
$prefs.streamingInsert
```

`True` なら OFF に戻して、OpenLess を再起動します。

## まだ自動化しないこと

定期実行タスクで勝手に本家をマージする運用はしません。設定やモデル順が意図せず変わる事故のほうが大きいからです。定期化するなら、まずは「本家更新があります」と通知するだけにします。
