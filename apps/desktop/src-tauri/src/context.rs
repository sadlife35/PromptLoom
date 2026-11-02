//! Context-aware transcript mode inference.
//!
//! Looks at the foreground application's executable name and returns the
//! transcript mode that fits it best. Used when the user has
//! `auto_context` enabled — the detected mode overrides their manual
//! Settings pick for that one dictation only.
//!
//! Lookup is a static exact-match on the EXE basename (case-insensitive).
//! Keep this list conservative: a wrong auto-switch is worse than no
//! auto-switch — the user then disables the feature and never trusts it
//! again. When unsure, return `None` and let the user's manual choice win.

use promptloom_commands::TranscriptMode;

/// Known process names, matched case-insensitively against the exe
/// basename (e.g. `Code.exe` matches `code.exe`). Order within a group
/// doesn't matter.
const CODE_EXES: &[&str] = &[
    // Primary targets
    "code.exe",            // VS Code
    "cursor.exe",          // Cursor
    "windsurf.exe",        // Windsurf
    "zed.exe",             // Zed
    // JetBrains family
    "idea.exe",
    "idea64.exe",
    "pycharm.exe",
    "pycharm64.exe",
    "webstorm.exe",
    "webstorm64.exe",
    "rustrover.exe",
    "rustrover64.exe",
    "goland.exe",
    "goland64.exe",
    "clion.exe",
    "clion64.exe",
    "rider.exe",
    "rider64.exe",
    "phpstorm.exe",
    "phpstorm64.exe",
    "rubymine.exe",
    "rubymine64.exe",
    // Microsoft
    "devenv.exe", // Visual Studio
    // Classic editors
    "sublime_text.exe",
    "notepad++.exe",
    "atom.exe",
    "gvim.exe",
    "neovide.exe",
];

const TERMINAL_EXES: &[&str] = &[
    "windowsterminal.exe",
    "cmd.exe",
    "powershell.exe",
    "pwsh.exe",
    "wezterm-gui.exe",
    "wezterm.exe",
    "alacritty.exe",
    "mintty.exe",   // Git Bash / MSYS
    "conemu64.exe",
    "conemu.exe",
    "conhost.exe",
    "tabby.exe",
    "hyper.exe",
];

/// Try to infer the right transcript mode from the foreground executable.
/// Returns `None` for anything we don't recognize — the caller falls back
/// to the user's manual choice.
pub fn infer_transcript_mode(exe: &str) -> Option<TranscriptMode> {
    let name = basename_lower(exe);
    if name.is_empty() {
        return None;
    }
    if TERMINAL_EXES.contains(&name.as_str()) {
        return Some(TranscriptMode::Terminal);
    }
    if CODE_EXES.contains(&name.as_str()) {
        return Some(TranscriptMode::Code);
    }
    None
}

fn basename_lower(path: &str) -> String {
    path.rsplit(['\\', '/'])
        .next()
        .unwrap_or(path)
        .to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vs_code_detected_as_code() {
        assert_eq!(
            infer_transcript_mode("Code.exe"),
            Some(TranscriptMode::Code)
        );
    }

    #[test]
    fn cursor_detected_as_code() {
        assert_eq!(
            infer_transcript_mode("Cursor.exe"),
            Some(TranscriptMode::Code)
        );
    }

    #[test]
    fn windows_terminal_detected_as_terminal() {
        assert_eq!(
            infer_transcript_mode("WindowsTerminal.exe"),
            Some(TranscriptMode::Terminal)
        );
    }

    #[test]
    fn powershell_detected_as_terminal() {
        assert_eq!(
            infer_transcript_mode("powershell.exe"),
            Some(TranscriptMode::Terminal)
        );
    }

    #[test]
    fn unknown_app_returns_none() {
        assert_eq!(infer_transcript_mode("notepad.exe"), None);
        assert_eq!(infer_transcript_mode("chrome.exe"), None);
        assert_eq!(infer_transcript_mode("slack.exe"), None);
    }

    #[test]
    fn case_insensitive() {
        assert_eq!(
            infer_transcript_mode("CODE.EXE"),
            Some(TranscriptMode::Code)
        );
        assert_eq!(
            infer_transcript_mode("cmd.EXE"),
            Some(TranscriptMode::Terminal)
        );
    }

    #[test]
    fn strips_path_prefix() {
        assert_eq!(
            infer_transcript_mode(r"C:\Users\x\AppData\Local\Programs\Microsoft VS Code\Code.exe"),
            Some(TranscriptMode::Code)
        );
        assert_eq!(
            infer_transcript_mode("/usr/bin/powershell.exe"),
            Some(TranscriptMode::Terminal)
        );
    }

    #[test]
    fn empty_exe_returns_none() {
        assert_eq!(infer_transcript_mode(""), None);
    }
}
