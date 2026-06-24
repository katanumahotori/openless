# OpenLess ビルド／日本語IME 落とし穴メモ（AI・他セッション向け）

最終更新: 2026-06-24（claude-opus-4-8）

このファイルは、OpenLess フォーク（`katanumahotori/openless`）を Windows 上でビルド・デバッグするときに
**何度もハマった落とし穴**を、他のセッション・他のAI（Codex 等）が再発見しなくて済むように残すもの。
ユーザー（片沼ほとり）はプログラミング不可。技術判断はAI側で完結させること。

関連: `OpenLess開発運用.md`（運用手順の正本）、`docs/upstream-japanese-ime-issues-prs.md`（IME問題の上流調査）

---

## 落とし穴①【最重要】ソースを直しても、起動するexeは自動更新されない

- ユーザーが普段起動するのは `openless-all/app/src-tauri/target/release/openless.exe`（デスクトップのショートカット）。
- **ソースを編集・コミットしただけでは、このexeは一切変わらない。** 手動で release 再ビルドが必要。
- 2026-06-24 の実例：「ストリーミングOFFなのに日本語が崩れる」バグ調査で、真因はコードではなく
  **稼働中exeが 6/21 19:29 ビルドの古い版**で、その後コミットした修正（上流1.3.11マージ・streaming opt-in・
  clipboard fallback・TSF無効化）が**1つも反映されていなかった**こと。
- 鉄則：**OpenLessのコードを直したら必ず release 再ビルドし、`target/release/openless.exe` の
  LastWriteTime が当日に更新されたことを確認してから「完了」と言う。** コード変更とビルドは常にワンセット。
- dev版（`target/debug/openless.exe`、`dev.ps1`）と普段使い（`target/release/openless.exe`）は別ファイル。
  直した版とユーザーが起動する版が一致しているか常に意識する。

## 落とし穴②【最重要】壊れた `msvcrt.lib` がリンクを殺す（__imp__wassert 未解決）

症状：
```
... \build\openless-<hash>\out\msvcrt.lib : warning LNK4003: invalid library format; library ignored
liblzma_sys-*.rlib(...) : error LNK2001: unresolved external symbol __imp__wassert
fatal error LNK1120: 1 unresolved externals
```
- **コンパイルは全部通るのに、最後のリンクだけ失敗する**のが特徴。
- 原因：`target/**/build/openless-<hash>/out/` に **82バイトの壊れた `msvcrt.lib`**（実体は `.drectve` だけの
  COFFオブジェクトで、ライブラリ書庫ではない）が残っていた。これがリンカの `/LIBPATH` 先頭に入り、
  `/defaultlib:msvcrt` の解決で本物の msvcrt.lib（UCRT、`__imp__wassert` を含む）を覆い隠す。
- **現在の `build.rs` はこのファイルを生成しない**（過去の build.rs の残骸。日付が古い＝May 8 / Jun 21 等）。
  だから消せば再生成されず、恒久的に直る。
- 直し方：
  ```bash
  find openless-all/app/src-tauri/target -name "msvcrt.lib" -delete
  ```
  （`target/**/build/openless-*/out/msvcrt.lib` を全部消す）→ 再ビルド。
- 「6/21は成功、今は失敗」のような不可解な回帰は、まずこの残骸を疑う。

## 落とし穴③ Bashツールからのビルドは MSVC リンク環境が欠ける／余計な小細工は逆効果

- このリポジトリを **Git Bash ツールから `cargo build` すると `LIB`/`INCLUDE` が未設定**。
  ただし rustc/cc が MSVC を自動検出するので、**コンパイルは通り、リンクも本来は通る**
  （落とし穴②さえ無ければ）。`LIB` 未設定そのものは主因ではなかった。
- **やってはいけない遠回り（2026-06-24 に実際に踏んだ）：**
  - `vcvars64.bat` を強制読み込み → 別バージョンのツールチェーンが有効化され、今度は **zstd-sys の
    コンパイルが壊れた**（`-fvisibility=hidden` 警告＋exit 2）。**vcvars は使わない。** rustc 自動検出が正しい。
  - Git Bash から `cmd //c build.bat` → cmd の作業ディレクトリ・引用符が壊れて "認識されません" になる。
  - `npm` は Bash ツールの PATH に無い（ユーザーの対話 PowerShell とは別環境）。
- 正しいビルド方法（どちらでも可、落とし穴②を消した後）：
  - フロント（`src/`）も変えたとき：PowerShell で `npm run tauri build`（正本手順）。
  - **Rust（`src-tauri/`）だけ変えたとき：`cargo build --release` で十分**（既存の埋め込み済みフロントを使う）。
    Bash からなら `export PATH="/c/Users/katan/.cargo/bin:$PATH" && cargo build --release`。
- 再ビルド前に**稼働中の `openless.exe` を kill**（exeをロックしているとリンク/出力で失敗）。

## 落とし穴④ 日本語が崩れる真因は「ストリーミング」ではなく TSF プロファイル切替

- ユーザー症状：「ストリーミング挿入をOFFにしているのに、入力時にストリーミングのように1文字ずつ崩れ、
  最後に確定した漢字の順序がおかしくなる」。
- 切り分け結果：
  - `streamingInsert=false` は設定・移行とも正しく効いていた（ストリーミング機能は無関係＝濡れ衣）。
  - 1文字ずつ SendInput する非ストリーミング経路（`insert_via_unicode_keystrokes`）は実質デッドコード。
  - OpenLess IME の TSF 登録はレジストリ上クリーン済み（過去のクリーンアップが有効）。
  - **真因：`windows_ime_session.rs::prepare_session()` が録音のたびに Windows の入力方式を
    OpenLess IME へ切り替え→復元しており、これが日本語の変換途中（特に最後の漢字）の順序を壊す。**
    上流に過去報告した症状（PR #362/#363、issue #665/#715 と同系統）そのもの。
- 対策（2026-06-24 実装済み）：`prepare_session()` 冒頭で `return PreparedWindowsImeSession::unavailable();`
  し、TSF 切替を**恒久無効化**。挿入は必ずクリップボード貼り付け（原子的＝順序が崩れない）に落ちる。
  `allowNonTsfInsertionFallback=true` 前提。

## 落とし穴⑤ 「無効化済み」のメモを信じない／上流マージで fork 改造が消える

- `OpenLess開発運用.md` の「改造の進捗」表には「TSF経路を完全無効化済み ✅ windows_ime_session.rs」と
  書いてあったが、**2026-06-24 時点で実コードには入っていなかった**（当該ファイルの最終変更は 6-02、
  6-21 の修正は無し）。上流 1.3.11 マージ（cfbd000）で消えた疑いが濃厚。
- 教訓：ドキュメントの「実装済み」を鵜呑みにせず、**実コードと `git log -- <file>` で必ず裏取り**する。
- `update_from_upstream.ps1` で rebase/merge した後は、fork 固有の以下が生きているか毎回確認：
  - TSF 切替の無効化（落とし穴④）
  - `streamingInsert` のデフォルト opt-in（false 保持）維持
  - 日本語フォント優先・TSF lang_id 等

---

## チェックリスト（OpenLess を直して反映するとき）

1. [ ] 稼働中 `openless.exe` を kill した
2. [ ] `target/**/build/openless-*/out/msvcrt.lib` の残骸が無いか確認（あれば削除）＝落とし穴②
3. [ ] `cargo build --release`（Rustのみ）or `npm run tauri build`（フロント含む）が成功
4. [ ] `target/release/openless.exe` の LastWriteTime が**当日**に更新された
5. [ ] 上流マージ直後なら fork 固有改造が生きているか確認＝落とし穴⑤
6. [ ] それから初めて「完了」と報告する
