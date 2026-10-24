# M4 dogfood — 30-run matrix

**Ship gate:** at least 28 of 30 hotkey loops start/stop correctly, 4 of 4
target apps receive text, zero audio files persist afterward.

Prerequisites:
- `.promptloom/terms.yml` configured in a project folder PromptLoom is
  pointed at (via Settings → Project vocabulary).
- Model loaded (`whisper model loaded at startup` in logs).
- Fresh app launch; nothing in the clipboard-history that overlaps with
  what you'll dictate.

## Hotkey loop (30 runs)

Pick any single target (Notepad is easiest). For each run: hold F8 for
2–4 seconds speaking a short phrase, release, confirm text appeared.
Tick each run that worked.

```
 1 [✓]    2 [✓]    3 [✓]    4 [✓]    5 [✓]
 6 [✓]    7 [✓]    8 [✓]    9 [✓]   10 [✓]
11 [✓]   12 [✓]   13 [✓]   14 [✓]   15 [✓]
16 [✓]   17 [✓]   18 [✓]   19 [✓]   20 [✓]
21 [✓]   22 [✓]   23 [✓]   24 [✓]   25 [✓]
26 [✓]   27 [✓]   28 [✓]   29 [✓]   30 [✓]
```

Pass if **≥ 28 / 30**.

## Target-app matrix (4 runs)

For each app: focus → hold F8 → say "the deploy pipeline runs on
Tuesday" → release → verify text landed verbatim (or vocabulary-corrected).

| Target             | Pass | Notes                     |
| ------------------ | ---- | ------------------------- |
| Notepad            | [ ]  |                           |
| Chromium input     | [ ]  | Google / GitHub search ok |
| VS Code editor     | [ ]  |                           |
| Windows Terminal   | [ ]  |                           |

Pass if **4 / 4**.

## Vocabulary determinism check

With a project pointed at a folder whose `.promptloom/terms.yml` contains:

```yaml
terms:
  "super base": "Supabase"
  "prompt loom": "PromptLoom"
  "post gress": "Postgres"
```

Dictate: *"Deploy prompt loom to super base backed by post gress."*

Expected post-correction: **"Deploy PromptLoom to Supabase backed by Postgres."**

Pass if the result matches on at least **3 consecutive runs**.

## Negative paths

| Scenario                      | Expected                                     | Pass |
| ----------------------------- | -------------------------------------------- | ---- |
| Hold F8 then Esc              | HUD closes; no delivery; no notice of paste  | [ ]  |
| Hold F8 < 250 ms              | "TooShort" notice; nothing delivered         | [ ]  |
| Hold F8 then Alt-Tab away     | `focus changed; copy-only` log; no injection | [ ]  |
| Hold F8 in UAC prompt         | copy-only (focus steal natural)              | [ ]  |

## Privacy audit

After completing the 30 hotkey loops, open Settings → "Run audit now".

Expected: **0 wav, 0 log files** in the scratch directory.

If anything shows up, dictation left residue — report + fix before shipping.
