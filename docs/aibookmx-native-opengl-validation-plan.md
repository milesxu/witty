# aibookmx Native OpenGL Validation Plan

Updated: 2026-06-01

## Read-Only Preflight

Host:

```text
xuming-AIBOOK-ABA14011
Linux 6.6.10 aarch64
```

SSH preflight:

| Check | Result |
| --- | --- |
| Rust toolchain | `/home/xuming/.cargo/bin/cargo`, `/home/xuming/.cargo/bin/rustc` |
| `~/src/witty` | missing |
| SSH `DISPLAY` | empty |
| SSH `WAYLAND_DISPLAY` | empty |
| SSH `XDG_SESSION_TYPE` | `tty` |
| Warp OpenGL launcher | present |

Warp OpenGL desktop entry:

```text
Exec=env WGPU_BACKEND=gl warp-terminal %U
```

## Interpretation

aibookmx is a good candidate for safe-host native OpenGL validation because Warp
already runs there with an explicit OpenGL launcher. However, the current SSH
session does not expose the graphical desktop, and the Witty worktree is not
present on that host.

Do not treat this SSH TTY as sufficient for real native window validation.

## Proposed Flow

1. Sync or clone Witty to `~/src/witty` on aibookmx.
2. Run non-graphical checks over SSH first:
   - `cargo test -p witty-render-wgpu`
   - `cargo test -p witty-app`
   - `cargo run -p witty-app -- --renderer-backend-info`
   - `cargo run -p witty-app -- --renderer-no-surface-diagnostics`
3. For real native window validation, launch from a graphical aibookmx session
   using the same launcher-level pattern as Warp:

```text
env WGPU_BACKEND=gl cargo run -p witty-app -- --window --window-startup-report --window-exit-after-ms 1200
```

4. If aibookmx has a working local X11/Xvfb path, try the screenshot harness
   there instead of on the Linux/M1000 host:

```text
WITTY_CAPTURE_MODE=xvfb scripts/capture-gui-diagnostics.sh target/gui-regression/aibookmx-opengl-xvfb.xwd
```

## Boundaries

- Do not run Chromium/WebGPU validation on the Linux/M1000 host.
- Do not assume SSH TTY can start native windows without display/session setup.
- Do not run real native probes on aibookmx until the worktree exists and the
  intended graphical session is clear.
