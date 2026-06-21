# Linux OpenGL Desktop Entry

Updated: 2026-06-21

This repo carries desktop entry templates at
`packaging/linux/dev.witty.Witty.desktop` and
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
Exec=env WGPU_BACKEND=gl WITTY_LOG=... ... --window
```

That keeps OpenGL selection visible at the launcher boundary instead of relying
on an interactive shell to export `WGPU_BACKEND`. `WITTY_LOG` keeps GNOME menu
launches writing levelled logs under the Witty state directory.

For a local user install, use:

```text
scripts/install-witty-local.sh --dry-run
scripts/install-witty-local.sh
```

The installer writes `~/.local/bin/witty`, installs hicolor icons named
`dev.witty.Witty`, and generates
`~/.local/share/applications/dev.witty.Witty.desktop` with an absolute binary
path in `Exec`. It also writes an installed-build marker at
`$XDG_STATE_HOME/witty/install-state.v1.json`, or
`~/.local/state/witty/install-state.v1.json` if `XDG_STATE_HOME` is unset.
Native window logs are written under `$XDG_STATE_HOME/witty/logs/`, or
`~/.local/state/witty/logs/` if `XDG_STATE_HOME` is unset. Override
`WITTY_DESKTOP_LOG` during install to change the desktop launcher's default
filter.
Running installed windows compare their startup build id with that marker and
show a native `Restart to update` action after a newer local install. The
restart path writes `restart-state.v1.<pid>.json` in the Witty state directory
and starts the installed binary with `--window --restore-state <path>`.

Restart restore is a Level A, best-effort relaunch. Witty restores grid size,
tab records, local launch command/cwd/safe environment metadata, and
profile-launched session metadata where available. It does not serialize
terminal text or preserve ordinary local PTY child process continuity. Use
tmux or a future persistent Witty daemon for lossless shell/process continuity.

The checked-in templates assume `witty` is available on `PATH` and are mainly
for review or downstream packaging.

For repo-local manual launches, prefer the script instead of installing the
desktop entry:

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

GNOME dock pinning uses the stable desktop id `dev.witty.Witty`. The desktop
entry sets `Icon=dev.witty.Witty`, `Terminal=false`, and
`StartupWMClass=dev.witty.Witty`; the native Linux window also sets its
Wayland app id and X11 class/instance to the same value.
