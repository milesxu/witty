# Linux OpenGL Desktop Entry

Updated: 2026-06-01

This repo carries an OpenGL-specific desktop entry template at
`packaging/linux/dev.witty.Witty.OpenGL.desktop`.
For day-to-day development from the repo, use
`scripts/run-witty-native-opengl.sh`; it uses `target/debug/witty` when
available, falls back to `cargo run -p witty-app -- --window`, and exports
`WGPU_BACKEND=gl` before launching the native window. Use
`scripts/run-witty-native-opengl.sh --print-command` to verify the selected
binary or cargo fallback without opening a window.
The same script can also run daily non-graphical helpers without appending
`--window`; for example, `--font-list nerd`, `--window-config-init`,
`--window-config-check`, `--window-config-effective`, and
`--renderer-backend-info` stay on the same OpenGL-safe binary selection path
while avoiding native window, PTY, surface, adapter, Chromium, and Vulkan work.

The template follows the same launcher-level backend policy observed on
`aibookmx` for Warp:

```text
Exec=env WGPU_BACKEND=gl ... --window
```

That keeps OpenGL selection visible at the launcher boundary instead of relying
on an interactive shell to export `WGPU_BACKEND`.

For a local user install, copy the desktop entry to:

```text
~/.local/share/applications/dev.witty.Witty.OpenGL.desktop
```

The checked-in entry assumes `witty` is available on `PATH`. If the binary
lives in a local install directory, replace `witty` in the `Exec` line with
that absolute path. For repo-local manual launches, prefer the script instead
of installing the desktop entry:

```text
scripts/run-witty-native-opengl.sh --print-command
scripts/run-witty-native-opengl.sh --font-list nerd
scripts/run-witty-native-opengl.sh --window-config-effective --no-window-config
scripts/run-witty-native-opengl.sh
scripts/run-witty-native-opengl.sh --window-startup-report --window-exit-after-ms 1500
WITTY_NATIVE_BIN=/path/to/witty scripts/run-witty-native-opengl.sh
```

The Linux/M1000 development host still pins the native renderer path to
`wgpu::Backends::GL` in code. The desktop entry is intentionally redundant: it
matches the visible operational practice used by Warp and makes accidental
backend drift easier to spot during manual launches.

The corresponding non-graphical policy check is:

```text
scripts/run-witty-native-opengl.sh --renderer-backend-info
```

It reports the native backend policy without opening a window or querying GPU
adapters, so it is safe for local verification after display-driver incidents.
