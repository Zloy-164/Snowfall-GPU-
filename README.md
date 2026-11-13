# Snowfall GPU — experimental Windows build

## What's changed
- Direct3D 11 + DirectComposition renderer and original snow physics.
- GPU load caps: default up to 30 FPS, tray toggle up to 45 FPS; 20 FPS in battery saver. It can skip rendering static frames.
- Snow piling on windows / at bottom, physically influenced by window motion; click-through window overlays.
- No auto-update checks or update URL opener, no installation binary, no first-run wizard, no Run-key autostart API.
- New app name / icon / own preferences namespace (`HKCU\Software\SnowfallGPU`) so it doesn't overwrite the original.
- The app itself has no intentional network features. A first-time Cargo **build** downloads Rust dependencies.
- User's TikTok reference also shows a snowman on the macOS Dock: this is **not yet implemented** in the Windows fork.

## Building on Windows 10/11 (64-bit)
1. Install Rust with rustup (https://rustup.rs). Default stable MSVC toolchain. Requires Rust 1.85+ (edition 2024).
2. Install Microsoft Visual Studio 2022 Build Tools with "Desktop development with C++" and a Windows 10/11 SDK.
3. Extract all files. Double-click `Build Snowfall GPU.cmd`.
4. After SUCCESS, launch the single `SnowfallGPU.exe` next to the build script. Rust and the build tools are **not** required to run the compiled .exe.
5. Tray snowflake icon: adjust intensity, falling opacity, window snow, bottom snow, performance toggle, or Exit.

## Power & safety
- 30 FPS is the default for low GPU usage; 45 FPS is optional. FPS is an upper bound, not a guarantee of smoothness.
- App pauses rendering on lock/sleep; dynamic skipping is retained. Task Manager's GPU usage can vary greatly by hardware and screen resolution.
- Build locally; **not compiled or runtime-tested on a Windows PC in this environment**.
- Do not run code as administrator. First time build requires internet to download crates. Cargo.lock pins dependency versions but does not by itself certify they are safe.
- No installed files or auto-run entries. To remove: close via tray, delete the EXE; optionally delete HKCU\Software\SnowfallGPU (only settings).
- This does not have the same custom color/height menus as our earlier C# Lite versions. Those would require additional GPU code.

## Optional build with GitHub Actions
Upload the full source tree to a GitHub repository and run the `Build Snowfall GPU for Windows` workflow manually (Actions tab). Download the artifact `SnowfallGPU-Windows`, containing the portable EXE. The Windows build has not been executed here.
