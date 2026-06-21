# Upstream Japanese/IME Related Issues and PRs

調査日: 2026-06-21

対象:

- upstream: https://github.com/Open-Less/openless
- fork: https://github.com/katanumahotori/openless

目的:

- 日本語 IME で文字順が崩れる問題、TSF/OpenLess IME、Windows 挿入、履歴、ストリーミング既定値に関する upstream の Issue / PR を整理する。
- 「本家に戻したら日本語まわりがまた壊れるか」「本家に投げるべき一般問題か」「自分の独自運用だけの問題か」を判断しやすくする。

## 結論

- 過去に片沼さんが出した日本語 IME 直接関連 PR は 2 本あり、どちらも未マージ。
  - #362 は「非中国語環境では TSF を避ける」方向で、実装方針が粗いとして受け入れられなかった。
  - #363 は「Unicode SendInput が日本語 IME の変換状態と競合して文字順が崩れる」問題を扱ったが、最終的に outdated として閉じられた。
- ただし本家側でも、TSF/OpenLessIme.dll が Windows の IME/Explorer/QQ/入力法に副作用を出す問題は複数報告されている。これは片沼さん固有ではなく、Windows 利用者一般に影響しうる問題。
- 日本語表示フォント問題は #364 が merged、その後 #368 で maintainer 側により復元されており、これは本家に入った扱い。
- 履歴読み込み失敗そのものに近い UI IPC 失敗問題は #310、履歴検索は #612、ASR 失敗時に履歴へ残らない問題は #613。#613 は本家で修正済み、#612 は未解決。
- 「アップデートで streaming insert が勝手に ON になる」問題は、本家の #439/#440/#442/#446 の方針と片沼さんの好みが衝突している。これは bug というより product default の違いなので、fork では独自防衛テストが必要。

## 片沼さんが出した PR

| PR | 状態 | 関連度 | 内容 | 判断 |
|---|---|---:|---|---|
| [#362](https://github.com/Open-Less/openless/pull/362) | closed / not merged | 高 | 日本語ホストで OpenLess TSF が中国語 profile を前提に動き、ATOK/MS-IME の状態を壊すため、非 zh では TSF を避ける案 | 問題提起は有効。ただし実装が stop-gap で、C++ DLL 側の lang_id 問題が未解決だったため upstream には通らなかった |
| [#363](https://github.com/Open-Less/openless/pull/363) | closed / not merged | 高 | `KEYEVENTF_UNICODE` の 1 文字ずつ SendInput が日本語 IME の変換状態と競合し、ひらがなと漢字の順序が崩れる問題 | 問題提起は有効。最終コメントは outdated。現在 fork で入れた clipboard fallback は、この問題へのより小さい実用対策 |
| [#364](https://github.com/Open-Less/openless/pull/364) | merged | 高 | Noto Sans JP 同梱と UI フォント選択 | 日本語 UI 表示の正攻法として採用済み |
| [#365](https://github.com/Open-Less/openless/pull/365) | closed / not merged | 中 | 翻訳機能の master toggle | 日本語 IME ではなく運用設定寄り。fork 独自運用なら保持候補 |
| [#366](https://github.com/Open-Less/openless/pull/366) | closed / not merged | 中 | ユーザー定義 polish style / system prompt | 日本語整形の自由度には関係するが、upstream には通っていない |
| [#367](https://github.com/Open-Less/openless/pull/367) | closed / not merged | 低-中 | アプリ別 polish mode 自動切替 | 便利機能。日本語 IME の根本問題ではない |
| [#375](https://github.com/Open-Less/openless/pull/375) | merged | 中 | 辞書エントリを Whisper prompt に渡す | 日本語固有語・固有名詞の改善として採用済み |
| [#376](https://github.com/Open-Less/openless/pull/376) | closed / not merged | 中 | cross-mode universal directives | 質問に答えない整形などの運用には関係するが、upstream には通っていない |
| [#569](https://github.com/Open-Less/openless/pull/569) | merged | 低 | マルチディスプレイで capsule が入力中画面を追う | 採用済み |
| [#572](https://github.com/Open-Less/openless/pull/572) | merged | 中 | Whisper verbose_json で幻聴 segment を捨てる | ASR 品質改善として採用済み |
| [#646](https://github.com/Open-Less/openless/pull/646) | merged | 中 | 長時間録音向け Cloud Whisper timeout 拡張 | 長文入力の安定性として採用済み |

片沼さん authored の upstream issue は見つからなかった。

## IME / TSF / 挿入の upstream Issue

| Issue | 状態 | 内容 | 片沼さん問題との関係 |
|---|---|---|---|
| [#20](https://github.com/Open-Less/openless/issues/20) | closed | Ctrl+V 送信成功だけで「挿入済み」と扱うのは危険 | fork の現在の未解決注意点にも近い。clipboard paste は「送った」だけで「入った」とは限らない |
| [#95](https://github.com/Open-Less/openless/issues/95) | closed | Windows で出力前に clipboard を無条件上書きし、復元しない | 本家でも一般問題として扱われた |
| [#96](https://github.com/Open-Less/openless/issues/96) | closed | current caret 保証がなく、盲目的に Ctrl+V している | 挿入先・履歴・実表示のズレに関係 |
| [#111](https://github.com/Open-Less/openless/issues/111) | closed | 挿入失敗時に dictation text が失われるため、clipboard 保持設定を追加 | 「失敗して履歴にも残らない」体験に近い |
| [#167](https://github.com/Open-Less/openless/issues/167) | closed | 連続 dictation で clipboard restore が古い内容を戻す競合 | 高頻度利用者ほど踏みやすい |
| [#207](https://github.com/Open-Less/openless/issues/207) | closed | Windows の Codex app で capsule が出ない | Codex Desktop 利用環境の相性問題 |
| [#288](https://github.com/Open-Less/openless/issues/288) | closed | QQ + Microsoft Pinyin で TSF DLL / CRT 周辺クラッシュ | TSF DLL がホストアプリ側で副作用を出す証拠 |
| [#469](https://github.com/Open-Less/openless/issues/469) | closed | 音声入力後、入力法が OpenLess IME から戻らない | 片沼さんの #362 と同系統。中国語圏以外にも IME 復元問題として一般性あり |
| [#481](https://github.com/Open-Less/openless/issues/481) | closed | 音声入力前のユーザー入力法を復元する | #469 系の修正タスク |
| [#525](https://github.com/Open-Less/openless/issues/525) | closed | 入力後にユーザー clipboard を復元してほしい | clipboard fallback と副作用の一般問題 |
| [#643](https://github.com/Open-Less/openless/issues/643) | open | 繁体中文ユーザーが簡体入力/出力に寄る | 日本語ではないが、言語・字形・IME 設定の保持問題 |
| [#665](https://github.com/Open-Less/openless/issues/665) | open | OpenLessIme.dll が explorer.exe / taskbar hang を起こす疑い | TSF をデフォルト有効にするリスクの強い根拠 |

## IME / TSF / 挿入の upstream PR

| PR | 状態 | 内容 | 判断 |
|---|---|---|---|
| [#210](https://github.com/Open-Less/openless/pull/210) | merged | Windows TSF IME insertion backend 追加 | TSF 経路の起点。日本語問題の根でもある |
| [#214](https://github.com/Open-Less/openless/pull/214) | closed | Windows TSF IME insertion support | #210 周辺の未採用案 |
| [#234](https://github.com/Open-Less/openless/pull/234) | closed | Windows IME abort restore ordering 修正 | IME restore 順序問題 |
| [#268](https://github.com/Open-Less/openless/pull/268) | merged | NSIS installer で Windows IME を登録 | TSF が実ユーザー環境に入る導線 |
| [#287](https://github.com/Open-Less/openless/pull/287) | merged | TSF DLL を静的 CRT リンクにして QQ などの DLL 衝突を回避 | #288 の系統。TSF 副作用への本家修正 |
| [#369](https://github.com/Open-Less/openless/pull/369) | merged | installer で TSF IME を置換前に unregister | upgrade 時の TSF 残骸対策 |
| [#377](https://github.com/Open-Less/openless/pull/377) | merged | Windows/Linux paste shortcut を設定可能化 | keyboard/paste 経路の柔軟化 |
| [#471](https://github.com/Open-Less/openless/pull/471) | merged | #466/#468/#469/#470 の Windows 高頻度フィードバック修正 | IME が戻らない問題を含む |
| [#502](https://github.com/Open-Less/openless/pull/502) | merged | Windows streaming SendInput events を buffer | streaming + SendInput の圧を下げる |
| [#503](https://github.com/Open-Less/openless/pull/503) | merged | dictation 後の SendInput pressure を減らす | 日本語 IME 順序問題に隣接 |
| [#513](https://github.com/Open-Less/openless/pull/513) | merged | Windows streaming SendInput を pace | streaming 挿入安定化 |
| [#514](https://github.com/Open-Less/openless/pull/514) | merged | Windows IME submit timeout を短縮 | IME 経路の安定化 |
| [#533](https://github.com/Open-Less/openless/pull/533) | merged | Windows IME Whisper fixes | 詳細要確認だが IME/ASR 周辺 |
| [#626](https://github.com/Open-Less/openless/pull/626) | merged | macOS で挿入後に clipboard 復元 | clipboard 副作用対策の横展開 |
| [#715](https://github.com/Open-Less/openless/pull/715) | open | Windows modifier と TSF input hangs 回避。TSF 登録/有効化をデフォルトで避ける方向 | 片沼さんの #362 と問題意識がかなり近い。今後取り込み候補 |

## 履歴 / 失敗保存の upstream Issue / PR

| Item | 状態 | 内容 | 判断 |
|---|---|---|---|
| [Issue #310](https://github.com/Open-Less/openless/issues/310) | closed | History page の IPC 失敗で loading が固まり、エラーが見えない | 以前こちらで直した履歴読み込み失敗と同じカテゴリ |
| [Issue #612](https://github.com/Open-Less/openless/issues/612) | open | History search box がクリック・検索できない | 未解決。履歴 UI 改善として追跡 |
| [Issue #613](https://github.com/Open-Less/openless/issues/613) | closed | ASR 失敗時に録音を履歴に残し、再転写できるようにする | こちらが求めた「長く喋った音声を失わない」に近い。本家修正済み |
| [PR #343](https://github.com/Open-Less/openless/pull/343) | merged | History IPC failure でフィードバックと loading 回復 | #310 系 |
| [PR #627](https://github.com/Open-Less/openless/pull/627) | merged | History search box をクリック・検索可能にする | #612 を閉じる想定だが issue は open のまま |
| [PR #637](https://github.com/Open-Less/openless/pull/637) | merged | ASR 失敗時の録音保持 + 履歴から再転写 | #613 の本修正 |

## Streaming default / 設定保持

| Item | 状態 | 内容 | 判断 |
|---|---|---|---|
| [Issue #439](https://github.com/Open-Less/openless/issues/439) | closed | 流式入力/出力を default on にしたいという要望 | 本家方針は片沼さんの好みと逆 |
| [Issue #440](https://github.com/Open-Less/openless/issues/440) | closed | 新装/更新後に streaming insert / output を default on | アップデート時に設定が戻る事故の元 |
| [PR #442](https://github.com/Open-Less/openless/pull/442) | merged | streaming-on by default | fork では要注意の差分 |
| [PR #446](https://github.com/Open-Less/openless/pull/446) | merged | migrated prefs で streaming insertion enabled を保つ | 「既存ユーザーも ON に寄せる」方向の補強 |

## こちらから upstream に出すなら

優先度高:

1. Windows 日本語 IME で Unicode SendInput が文字順を崩す問題  
   新規 Issue として再提出する価値あり。#363 は未マージだが、現在は #715 の流れで TSF/SendInput の副作用が本家でも問題視されているため、以前より通りやすい。

2. TSF / OpenLessIme.dll をデフォルト登録・有効化するとホストプロセスに副作用が出る問題  
   #665/#715 が開いているので、新規 Issue より既存 PR/Issue への日本語環境再現情報追加がよい。

3. 既存設定をアップデートで勝手に streaming on へ戻さないこと  
   本家の product default と衝突するため、Issue 化するなら「default は本家方針でよいが、ユーザーが明示的に off にした設定は移行で尊重してほしい」と切るのがよい。

優先度中:

4. 履歴・失敗保存・再転写まわり  
   #613/#637 が入ったので、fork 側の独自修正を最小化できる可能性あり。ただし実機で「失敗時に履歴に残る」「再転写できる」を確認してから本家へ戻す。

5. 日本語 UI 表示  
   #364/#368 が入っているので、Noto Sans JP/フォント選択の基本は本家側で済んでいる。fork 独自保持は必要最小限でよい。

upstream に投げにくいもの:

- Groq のモデル優先順位、無料枠フォールバック、質問に答えない polish prompt などは、片沼さんの運用・日本語執筆スタイルに強く依存する。Issue より fork 独自設定として守る方がよい。

## fork 側で守るべき回帰テスト

- streaming insert を明示 off にした既存設定が、upstream merge 後も off のまま残る。
- 日本語テキストを Windows 挿入 fallback に通しても、1 文字ずつ Unicode SendInput 経路に戻らない。
- TSF/OpenLessIme.dll が既定で勝手に再登録・再有効化されない。
- History IPC 失敗時に loading で固まらず、エラーが見える。
- ASR / polish 失敗時に、少なくとも音声または raw transcript が履歴・ログに残る。

