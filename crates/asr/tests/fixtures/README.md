# ASR fixture WAVs

The ten M2-mandated cases live here. They are **user-consented recordings**
and are NOT committed to this repository (gitignored). Record them yourself
on your own machine and drop them into this directory.

## Protocol

| # | File              | Prompt (read aloud)                                                | Category        |
|---|-------------------|--------------------------------------------------------------------|-----------------|
| 1 | `silence.wav`     | *(record 1 s of silence)*                                          | silence         |
| 2 | `short.wav`       | "hello world"                                                      | short prose     |
| 3 | `prose.wav`       | "Open the terminal and run the test suite before pushing."         | short prose     |
| 4 | `dev-terms.wav`   | "Deploy the service to Supabase using the Postgres backend."       | developer terms |
| 5 | `dev-stack.wav`   | "Rust, TypeScript, and Tauri version two."                         | developer terms |
| 6 | `correction1.wav` | "Push the PromptLoom branch and open a pull request."              | correction      |
| 7 | `correction2.wav` | "Super Base is our auth provider."                                 | correction      |
| 8 | `noisy.wav`       | *(same as #3 with background keyboard typing)*                     | moderate noise  |
| 9 | `long.wav`        | *(read any 10 s paragraph)*                                        | long prose      |
|10 | `pause.wav`       | *(short sentence with a mid-sentence pause of ~1 s)*               | pauses          |

Record at any device-native sample rate; the loader downmixes to mono and
resamples to 16 kHz PCM16.

## Expectations

[`expected.json`](./expected.json) in this directory holds the expected
transcript per file, plus the normalizations that are permitted when
comparing against whisper's output (lowercase, trim punctuation, collapse
whitespace, Levenshtein tolerance for `moderate noise` and `long prose`).

## Running the harness

```powershell
cargo test -p promptloom-asr --tests -- --ignored
```

The `#[ignore]` is intentional: fresh checkouts and CI should pass without
these fixtures. Only developers who have recorded them run the harness.
