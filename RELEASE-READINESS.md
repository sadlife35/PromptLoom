# Release readiness

Three things gate v0.1 ship: smoke-test on a clean VM, signed installer,
and the error-recovery audit (code done — needs manual verification).

## 1. Smoke-test checklist (fresh Windows VM)

Run **in order** on a vanilla Windows 11 VM with no dev tools installed.
Each step has a pass criterion. Stop and file a bug on the first fail.

### A. Install
1. Copy `PromptLoom_0.1.0_x64-setup.exe` to the VM. Double-click.
   - **Pass**: NSIS installer opens. **Fail criterion**: SmartScreen red
     wall with no "Run anyway" link (only present until the installer is
     signed — see §2).
2. Click through install. Launch from Start menu.
   - **Pass**: Settings window opens, onboarding wizard shown on step 1 of 6.

### B. Onboarding
3. **Privacy** — click Continue. **Pass**: advances.
4. **Microphone** — pick the default mic, speak, watch the level bar move.
   - **Pass**: bar reacts within 200ms of speech.
   - **Known dead-end**: if Windows mic privacy is off, no devices show.
     The notice tells the user what to do; verify the wording is correct.
5. **Local model** — click **Download model** (default: ggml-small.en, ~466 MB).
   - **Pass**: progress bar fills, "SHA-256 verified" pill appears.
   - **Fail**: hash mismatch pill → the download was corrupted. Retry.
   - **Fail**: "Whisper engine missing" banner → `whisper-server.exe` is
     not next to the model and not in a `tools/` ancestor. Fix per §2.
6. **Shortcut** — leave as `CapsLock`. Continue.
7. **Vocabulary** — skip. Continue.
8. **Finish** — close the window.

### C. First dictation
9. Open Notepad. Click into the text area to focus it.
10. Hold `CapsLock`, say *"this is a smoke test"*, release.
    - **Pass within 2s**: text appears in Notepad.
    - **Pass within 400ms**: HUD pill shows a partial transcript (ghost
      text above pill) — this verifies streaming partials.
11. Hold `CapsLock`, say anything, press `Esc` while still holding.
    - **Pass**: HUD shows "Cancelled", nothing is pasted.

### D. Error recovery (the real tests)
12. **Kill whisper-server mid-dictation**: Task Manager → kill
    `whisper-server.exe` → immediately hold `CapsLock` and speak.
    - **Pass**: HUD shows a fallback notice, next dictation re-spawns the
      server (watch Task Manager — new PID). Confirms backoff + restart
      from `crates/asr/src/whisper.rs`.
13. **Delete the model file**: Settings → Remove model → confirm.
    - **Pass**: app returns to onboarding at the model step. Do **not**
      allow hotkey to fire cleanly without a model.
14. **Revoke mic permission**: Windows Settings → Privacy → Microphone →
    turn off for PromptLoom → hold hotkey.
    - **Pass**: HUD shows an error with mic-permission hint, does not crash.
15. **Target window changed**: Open Notepad, focus it, hold `CapsLock`,
    while still holding Alt+Tab to another window, release.
    - **Pass**: text is copied to clipboard only (no keystroke injection
      into the wrong window). Verify `ClipboardAndHudSink::deliver`
      target-identity re-check is working.

### E. Settings round-trip
16. Change hotkey, close app, reopen → new hotkey persists.
17. Open Dictionary, add `super base` → `Supabase`, close app, reopen →
    term still there, and dictating "super base" produces `Supabase`.

---

## 2. Signed installer

Unsigned installers hit SmartScreen's red wall → users can't run them →
zero adoption. Blocked only on a code-signing certificate purchase.

### Options (cheapest first)

| Option | Cost | Setup |
|---|---|---|
| **Azure Trusted Signing** | ~$10/mo | No hardware token. Microsoft verifies org, then CI signs via REST. Best for CI. |
| **Sectigo OV Code Signing** | ~$200/yr | USB token (shipped physically) OR cloud HSM. OV, so SmartScreen warms up after ~3k installs. |
| **DigiCert EV Code Signing** | ~$450/yr | USB token or cloud HSM. EV, so SmartScreen trusts immediately. |

**Recommended**: Azure Trusted Signing — cheapest, CI-friendly, no token.

### Once you have the cert

Tauri reads signing from `tauri.conf.json > bundle.windows`. Two shapes:

**For installed cert (USB token or Windows cert store)**:
```json
"windows": {
  "certificateThumbprint": "YOUR_SHA1_THUMBPRINT_HERE",
  "digestAlgorithm": "sha256",
  "timestampUrl": "http://timestamp.sectigo.com"
}
```

**For Azure Trusted Signing (custom signCommand)**:
```json
"windows": {
  "signCommand": "signtool sign /tr http://timestamp.acs.microsoft.com /td sha256 /fd sha256 /dlib 'C:\\path\\to\\Azure.CodeSigning.Dlib.dll' /dmdf 'C:\\path\\to\\metadata.json' %1"
}
```

Do **not** commit the thumbprint — put it in a CI secret and inject at
build time via `TAURI_SIGNING_PRIVATE_KEY` + a custom build script, or
patch `tauri.conf.json` from CI.

The `digestAlgorithm` and `timestampUrl` placeholders are already
in `apps/desktop/src-tauri/tauri.conf.json`.

### Bundle whisper-server.exe with installer

Current flow expects `whisper-server.exe` next to the model or in a
`tools/` folder. For v0.1 ship, two options:

- **A.** Bundle via Tauri `externalBin` — rename the binary to
  `whisper-server-x86_64-pc-windows-msvc.exe`, put it under
  `apps/desktop/src-tauri/binaries/`, add
  `"externalBin": ["binaries/whisper-server"]` to the bundle config.
  Also update `resolve_engine_path()` to check Tauri's resource dir.
- **B.** Download on first run — add a second download step to onboarding
  that fetches `whisper-server.exe` from the whisper.cpp releases page,
  SHA-verifies, stores alongside models. Keeps installer small.

**Recommended**: **A** (bundle). Users already accept a 466 MB model
download; a 5 MB binary bundled in the 30 MB installer is a non-issue
and removes a failure mode.

---

## 3. Error-recovery audit result

Code paths exist for every failure mode in §D above:

- **Model missing** → `set_active_model()` swaps to `Placeholder`
  transcriber, next dictation shows a clear error (main.rs:942-953).
- **Model SHA mismatch** → `verify_file()` fails, onboarding shows
  "hash mismatch" pill (main.rs:875, Onboarding.tsx:415).
- **whisper-server dies** → `retry_backoff()` 5s/15s/60s, next
  dictation re-spawns (crates/asr/src/whisper.rs).
- **whisper-server binary missing** → **now fixed**: Onboarding blocks
  Continue and shows the missing-engine notice (Onboarding.tsx).
- **Target window changed** → `dispatch_selection()` + ClipboardAndHudSink
  re-check HWND + PID before `send_ctrl_v()`.
- **Mic permission revoked** → `cpal` returns a device error, surfaced
  via `Notice::Error`.

**None of these have been tested on a real fresh install.** Code
existing is not the same as code working. The smoke test in §1 is
non-negotiable before cutting v0.1.
