# Browser Resize And DPI

Updated: 2026-05-29

## Purpose

The browser terminal needs one explicit resize path that keeps the canvas
backing store, wgpu surface, terminal grid, transport size, and frame-planning
metrics in sync.

## Data Flow

JavaScript owns browser layout measurement:

```text
canvas.getBoundingClientRect()
window.devicePixelRatio
```

The page computes and applies the canvas backing size:

```text
canvas.width = ceil(css_width * device_pixel_ratio)
canvas.height = ceil(css_height * device_pixel_ratio)
```

Then it calls:

```text
WittyWebSession::resize(css_width, css_height, device_pixel_ratio)
```

Rust recomputes:

- backing width and height
- cell metrics scaled by `devicePixelRatio`
- terminal grid size

Then Rust applies the resize through:

- `WgpuRectRenderer::resize(backing_width, backing_height)`
- `TerminalApp::set_cell_metrics(scaled_metrics)`
- `BasicTerminal::resize(grid)`
- `TerminalApp::resize_transport(grid)`

## DPR Rule

Cell metrics are scaled by DPR before grid calculation. This keeps the logical
terminal grid stable when backing pixels increase for high-DPI displays.

Example:

```text
CSS: 450 x 180
DPR: 2
backing: 900 x 360
cell: 18 x 36
grid: 9 rows x 48 cols
```

This is equivalent to computing the grid from CSS pixels with the default
`9 x 18` cell size.

## Runtime Smoke

`scripts/run-witty-web-smoke.sh` changes the canvas CSS size to `720 x 360`,
calls `window.wittySyncCanvasSize()`, and verifies:

- backing width and height are updated
- grid rows and cols are positive
- `BrowserGatewayTransport` size matches the terminal grid
- the resized canvas screenshot is nonblank
