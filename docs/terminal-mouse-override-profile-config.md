# Terminal Mouse Override Profile Config

Updated: 2026-05-30

m109 exposes the m108 mouse selection override policy through the first
profile/config surface.

## Policy Values

`shift-select` is the product default.

| Value | Behavior while application mouse reporting is active |
| --- | --- |
| `shift-select` | Shift-left drag stays local to terminal selection; native Shift-middle pastes primary selection; Shift-wheel scrolls local scrollback in native and browser modes. |
| `disabled` | Shift mouse gestures are sent to the application as raw xterm mouse reports with the Shift modifier bit. |

## Native Window

The native window entry accepts:

```bash
witty --window --mouse-selection-override shift-select
witty --window --mouse-selection-override disabled
```

When omitted, the policy is `shift-select`.

## Browser Launcher

The product browser launcher accepts the same flag:

```bash
witty --web --mouse-selection-override disabled
```

`witty --web` serializes the value into the one-use browser session config
as `mouse_selection_override`. Browser JavaScript defaults missing values to
`shift-select` and rejects unknown values.

Browser parity covers Shift-left local selection and Shift-wheel local
scrollback. Browser primary-selection paste remains deferred because the current
web surface does not expose a system primary selection.

Browser clipboard follow-up work is planned in
`docs/browser-selection-clipboard-plan.md`.

## Validation

- `witty-app` parses the flag once and forwards it to native window mode or the
  web launcher.
- `witty-launcher` validates the same values before writing session config.
- The node browser smoke verifies both default `shift-select` local selection
  and `disabled` raw xterm Shift mouse reporting.
- The browser smoke verifies that Shift-wheel scrolls local scrollback under
  `shift-select` and that plain wheel continues to emit xterm wheel reports
  when mouse reporting is active.
