# Upstream Issue Drafts: Japanese IME / Windows Insertion

調査日: 2026-06-21
Claude Code review: 2026-06-21, `claude-opus-4-8`, completed

目的:

- Open-Less/openless upstream に出す Issue / コメント文面の下書き。
- 過去 PR #362 / #363 のように「こちらの実装を採用してほしい」という形ではなく、再現症状・影響・期待動作を中心にする。

## Review Summary

Claude Code 4.8 レビューの結論:

- Draft 1 は新規 Issue として出す価値あり。ただし、実機で崩れた actual text の例を 1 つ入れるまで投稿しない。
- Draft 2 はまず #665 にコメントするのがよい。#715 は PR なので、必要なら参照に留める。
- Draft 3 は新規 Issue として出さない。#442/#446 で upstream が streaming-on migration を意図的に決めているため、Issue 化すると蒸し返しに見える。必要なら #446 への質問コメントにする。

## Draft 1: New Issue

Title:

```text
[windows][ime] Japanese IME composition can reorder dictated text when Windows insertion falls back to Unicode SendInput
```

Body:

```markdown
### Summary

On Japanese Windows environments, dictated Japanese text can be inserted in the wrong visual order when OpenLess uses the Windows fallback insertion path based on per-codepoint Unicode `SendInput`.

### Environment

- OS: Windows 11
- Input methods observed: Microsoft IME / ATOK
- Language: Japanese
- Target apps: Electron/Chromium-style text fields and normal Windows text fields
- OpenLess versions affected: observed around the Windows TSF / SendInput insertion path; exact first affected version unknown

### Symptom

When dictating Japanese text, the final inserted text can appear reordered.

For example, kana that should remain in the IME composition flow may be delayed or left in the composition window, while kanji / ASCII characters are inserted earlier. The visible result is that parts of the sentence appear in a different order from the recognized/polished final text.

This is especially visible with mixed Japanese text containing hiragana, kanji, punctuation, or ASCII.

### Steps to reproduce

1. Use Windows 11 with the active IME set to Microsoft IME or ATOK, language Japanese.
2. Make the TSF path unavailable or disabled so insertion falls back to Unicode `SendInput`.
3. Dictate a mixed Japanese sentence, for example:

   ```text
   今日はAPIを3回呼んだ。
   ```

4. Observe the text inserted into the target app.

Expected:

```text
今日はAPIを3回呼んだ。
```

Actual:

```text
<paste one real reordered output from a Japanese Windows machine before posting>
```

### Expected behavior

The text inserted into the target application should match the final recognized/polished text order exactly.

OpenLess should avoid insertion methods that compete with the active Japanese IME composition state.

### Possible cause (unconfirmed)

My current understanding is:

- The fallback insertion path sends text through Unicode `SendInput` one codepoint at a time.
- Japanese IMEs maintain an active composition state.
- Per-codepoint synthetic Unicode input can be interpreted through or alongside that composition state instead of behaving like an atomic text insertion.
- This can make the target app receive committed characters and still-composing characters in a different order.

I may be wrong about the exact root cause, but the user-visible failure is that the visible inserted text order differs from the final text OpenLess intended to insert.

### Related upstream history

I see maintainers already note in #440 that CJK/Japanese IME can intercept insertion. This issue is to track the specific text-ordering failure mode, not just hang/interception.

- #210 introduced the Windows TSF IME insertion backend.
- #288 reported a TSF DLL host-process crash, later fixed.
- #469 / #481 tracked cases where OpenLess IME was not restored to the user's previous input method.
- #502 / #503 / #513 adjusted Windows streaming `SendInput` pacing / pressure.
- #665 reports OpenLessIme.dll causing explorer/taskbar hangs.
- #715 is currently addressing Windows modifier / TSF input hangs.
- My older PR #363 attempted to address this by changing the insertion path, but that implementation was not a good upstreamable fix and is now outdated. I am opening this issue to track the problem itself, not to insist on that implementation.

### Possible acceptance criteria

- Given the repro string above and Microsoft IME / ATOK active, the inserted text equals the final recognized text byte-for-byte.
- Japanese dictation insertion should not reorder text when the active Windows IME is Microsoft IME or ATOK.
- If the TSF path is unavailable or disabled, the fallback should use an insertion method that does not interact badly with Japanese IME composition.
- If OpenLess cannot verify insertion success, it should preserve the final text somewhere recoverable instead of silently losing it.

### Notes

This may overlap with #665 / #715 if the long-term fix is to avoid registering or activating OpenLess TSF by default and rely on a safer fallback. I am happy for this issue to be closed as duplicate if maintainers prefer to track the problem there.
```

Posting gate:

- 実機で「Actual」に入れる崩れた出力例を 1 件取るまで投稿しない。

## Draft 2: Comment for #665

Target:

- Primary: https://github.com/Open-Less/openless/issues/665
- Reference only if useful: https://github.com/Open-Less/openless/pull/715

Comment:

```markdown
Adding one data point from a Japanese Windows environment:

I previously hit a related class of problems where OpenLess' Windows IME / TSF / fallback insertion path interacted badly with Japanese IME composition.

The most visible symptom was not only hangs or input-method restoration, but text order corruption during Japanese dictation:

- active IME: Microsoft IME / ATOK
- language: Japanese
- symptom: kana remains in or is delayed by the IME composition flow while kanji / ASCII is inserted earlier, so the final visible text order differs from the final recognized/polished text

I had tried to address this in older PRs (#362 / #363), but those implementations were not good upstreamable fixes. I am not asking to revive those PRs.

I just want to note that, from the Japanese IME side, disabling or avoiding default TSF/OpenLessIme.dll activation looks like the safer direction. A fallback based on clipboard paste or another atomic insertion method appears less likely to compete with Japanese IME composition than per-codepoint Unicode `SendInput`.

If #665 remains the main tracking point for OpenLessIme.dll / TSF host-process side effects, this Japanese IME text-order case may be worth including in the test matrix. It also looks related to the direction being explored in #715.
```

## Draft 3: Question Comment for #446

Do not open this as a new issue. If needed, ask this as a short question on #446.

Target:

- https://github.com/Open-Less/openless/pull/446

Comment:

```markdown
#446 added a migration marker and, per its description, preserves manual opt-outs after migration.

Question: for users who set `streaming_insert=false` before the marker existed, is the one-time flip to `true` intended?

On Windows + Japanese IME, one-shot insertion was more reliable for me than per-keystroke / non-atomic insertion paths, including the Windows Unicode `SendInput` fallback. So I would prefer an explicit prior opt-out to survive upgrades.

I am asking whether this is by design, not requesting a default change for new users.
```

## Recommendation

1. まず Draft 2 を #665 にコメントする。
2. その後、maintainer が「別 Issue にして」と言うか、実機の actual reordered output を取れたら Draft 1 を新規 Issue として出す。
3. Draft 3 は新規 Issue にしない。必要になった場合だけ #446 への質問コメントにする。

