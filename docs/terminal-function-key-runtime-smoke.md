# Terminal Function-Key Runtime Smoke

Updated: 2026-05-30

The browser smoke now verifies representative function/navigation key encoding
through the real browser DOM to wasm session to gateway-input path.

The smoke dispatches synthetic `KeyboardEvent("keydown")` events on the
terminal canvas and checks the resulting gateway input frames.

Covered sequences:

| Browser event | Expected bytes |
| --- | --- |
| `Home` in normal cursor-key mode | `ESC [ H` |
| `Home` after `CSI ? 1 h` | `ESC O H` |
| `Insert` | `ESC [ 2 ~` |
| `F1` | `ESC O P` |
| `F5` | `ESC [ 15 ~` |
| `Shift+ArrowUp` | `ESC [ 1 ; 2 A` |
| `Ctrl+ArrowLeft` | `ESC [ 1 ; 5 D` |
| `Alt+Home` | `ESC [ 1 ; 3 H` |
| `Shift+F1` | `ESC [ 1 ; 2 P` |
| `Ctrl+F5` | `ESC [ 15 ; 5 ~` |

The function-key smoke runs after resize and screenshot checks. This keeps the
control-sequence input from interfering with later product-launcher resize
operations when the smoke is connected to a real PTY-backed shell.

Verification:

- `node --check scripts/run-witty-web-smoke.mjs`
- `scripts/run-witty-web-smoke.sh`
- `WITTY_WEB_SMOKE_GATEWAY=launcher scripts/run-witty-web-smoke.sh`
