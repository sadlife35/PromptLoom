# Icons

Tauri's bundler needs four icon files in this directory before
`cargo tauri build` will succeed:

```
32x32.png
128x128.png
128x128@2x.png
icon.ico
```

**You don't need to make them by hand.** The Tauri CLI generates all four
from one square source PNG (≥ 1024×1024, transparent background preferred):

```powershell
cd apps/desktop
cargo tauri icon path\to\your-logo.png
```

That writes all required files into this directory and additional sizes
Tauri uses for taskbar and system tray.

If you don't have a logo yet, any colored square PNG works for the alpha —
even a solid-color 1024×1024 placeholder. You can regenerate from a real
logo later and rebuild.

The actual icon files are gitignored (they're generated artifacts).
