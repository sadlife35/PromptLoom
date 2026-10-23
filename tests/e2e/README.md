# End-to-end smoke harness (Windows-only)

M3+ territory. In M0 this directory only holds the planned manual-test
protocol so later milestones have a stable checklist to run against.

## Manual Windows smoke matrix

For each target, run: normal delivery, Escape cancel, target switch,
model-missing error, microphone-denied error.

| Target             | Normal | Escape | Switch | ModelMissing | MicDenied |
| ------------------ | ------ | ------ | ------ | ------------ | --------- |
| Notepad            |        |        |        |              |           |
| Chromium input     |        |        |        |              |           |
| VS Code editor     |        |        |        |              |           |
| Windows Terminal   |        |        |        |              |           |

**Ship gate:** 30 manual trials, ≥ 28 start/stop correctly, zero audio files
persist. Scan `%APPDATA%\PromptLoom\` and the user's temp directory after the
30th trial — no `.wav`, `.pcm`, or transcript files must be present.
