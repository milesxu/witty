# Retained Planner Overlay Damage

Updated: 2026-05-30

`m174-retained-planner-overlay-damage` closes the R4 damage-contract coverage
gap for overlay-only frame changes.

## Implemented

- Added retained planner coverage for selection-only changes with
  `DamageRegion::Rows(vec![])`.
- Added retained planner coverage for cursor-only changes with
  `DamageRegion::Rows(vec![])`.
- Both tests assert terminal rows are reused, no terminal rows are rebuilt, and
  the overlay state is still present in `FrameStats`.

## Boundary

This pass does not change the damage model or terminal snapshot shape. It
documents the intended contract with tests: selection and cursor changes are
dynamic overlays, not terminal row mutations.

## Verification

Covered by:

- `cargo test -p witty-render-wgpu retained_planner_reuses_rows_for_selection_only_changes --quiet`
- `cargo test -p witty-render-wgpu retained_planner_reuses_rows_for_cursor_only_changes --quiet`
- `cargo test -p witty-render-wgpu --quiet`
