# 上流 issue / PR 下書き：履歴で「整形が空」のとき原文をコピーできない

作成: 2026-06-26（claude-opus-4-8）
対象上流: Open-Less/openless（remote `upstream` = appergb→Open-Less にリダイレクト）

## これは何か（日本語の説明）

- 症状：light（軽整文）等で録音→整形したとき、まれに**整形結果 `finalText` が空（0文字）**になる。識別原文 `rawTranscript` は完全に残っているのに、`errorCode` は `null`（エラー扱いされない）。
- 履歴詳細画面：左「原文」は全文表示されるのに、右上「コピー」は `finalText`（空）をコピーするので**画面に見えている原文を取り出せない**。「再転写」ボタンは「転写失敗」時しか出ず、本件は転写成功なので出ない／出ても無意味。
- つまり「原文は見えるのに UI から取り出せない」データ可取回性の穴。

## 既存との関係（重複回避メモ）

- Issue **#653**（open）＝「整形結果が原文と完全一致（整形が効かない）＋リトライ導線が無い」。
- PR **#666 / #694**（いずれも closed・未マージ）＝「履歴での再整形（repolish）」。Android自動更新等と抱き合わせで巨大だったため閉鎖。作者は単独PRで出し直すと表明（未提出）。
- 本件は **`finalText` が空・`errorCode` 未設定**の、より基礎的な「原文取り出し不能」問題。再整形の大機能とは独立に、小さく直せる。**重複しない。**

---

## Issue 投稿用（中国語・コピペ用）

タイトル:

```text
[area:ux] 润色为空（finalText 为空）时，历史页「复制」复制到空字符串，识别原文无法从 UI 取回
```

本文:

```markdown
### 现象

- 使用非「原文」输出模式（如「轻整文 / light」）完成一次语音转写 + 润色。
- 偶发：润色结果 `finalText` 为空（0 字），但 `rawTranscript`（识别原文）完整存在，且 `errorCode` 仍为 `null`（未被判定为错误）。
- 历史页详情：左侧「原文」正常显示全文；右侧润色结果为空白；底部显示「0 字 · 已复制，请 Ctrl+V」。
- 右上「复制」按钮复制的是 `finalText`（空串），用户**无法从 UI 复制到左侧可见的识别原文**。
- 「重新转录」按钮仅在 `errorCode === transcribeFailed/emptyTranscript` 且有录音时出现：本例 ASR 成功、`errorCode` 为 null，按钮不出现；即便出现也无济于事（失败的是润色而非转录）。
- 结果：识别原文在 UI 中可见却取不回；若录音已被 retention / 条数 cap 清理，只能手动去翻 `history.json`。

### 复现

1. 输出模式设为「轻整文 / light」。
2. 录一段较长内容（本例约 86 秒）。
3. 当本次润色返回空时，进入历史页查看该条。

### 证据

- 平台：Windows；输出模式：light；时长约 86s。
- `history.json` 该条：`finalText: ""`，`rawTranscript`（约 353 字）完整，`insertStatus: "copiedFallback"`，`errorCode: null`。
- 截图：原文面板有完整内容，润色面板空白，底部「0 字」。

### 期望

- 「复制」在 `finalText` 为空时回退复制 `rawTranscript`，不要复制空串。
- 原文面板提供独立「复制」入口，使识别原文随时可取回。

### 备注

- 与 #653（润色结果与原文一致）同属「润色产物不可用 + 缺少取回/重试入口」，但本例是 `finalText` 为空、`errorCode` 未标记，属更基础的「原文可取回性」问题，可独立修复，不依赖正在推进的「历史重新润色」。
```

---

## PR 投稿用（中国語・コピペ用）

> ⚠️ PR は **`upstream/beta` から切ったブランチ**に同じ差分を当てて出すこと（fork の backup ブランチからではない）。差分は `src/pages/History.tsx` のみで小さく、upstream/beta にもそのまま当たる想定。

タイトル:

```text
fix(history): 润色为空时「复制」回退到原文 + 原文面板独立复制入口
```

本文:

```markdown
## 摘要

修复：当润色结果为空（`finalText` 为空）时，历史页右上「复制」按钮复制的是空字符串，导致 UI 中可见的识别原文无法取回。

## 改动（仅前端，无后端/IPC 改动）

- `src/pages/History.tsx`
  - `onCopy`：`finalText` 去空白后为空时，回退复制 `rawTranscript`。
  - 原文面板新增独立「复制」按钮（仅在 `rawTranscript` 非空时显示），复制识别原文。
  - 复用现有 `common.copy` / `common.copied`，不新增 i18n。

## 不做什么

- 不实现「历史重新润色」——该功能由 #666 / #694 推进，避免重复与合并冲突。本 PR 只解决「原文取不回」这一基础可取回性问题，与 #653 互补。

## 测试

- `tsc --noEmit` 通过。
- 本地 release 构建通过。
- 在一条 `finalText` 为空的历史记录上：右上「复制」与原文面板「复制」均能取回识别原文；正常记录行为不变（「复制」仍复制润色结果）。
```

---

## 実際のコード差分（参考・src/pages/History.tsx）

1. `onCopy` の writeText を `item.finalText.trim() ? item.finalText : item.rawTranscript` に。
2. `onCopyRaw` ハンドラ追加（`rawTranscript` をコピー）。
3. 原文パネルの見出し行に独立コピーボタンを追加（`rawTranscript` がある時のみ表示）。
4. 状態 `justCopiedRaw` を追加。
