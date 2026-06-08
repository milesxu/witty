# Bundled Font Selection

Updated: 2026-05-30

## Decision

The initial product web distribution bundles DejaVu Sans Mono as
`witty-mono.ttf`.

Repo-controlled files:

```text
crates/witty-web/assets/fonts/witty-mono.ttf
crates/witty-web/assets/licenses/dejavu-fonts.txt
```

## Rationale

DejaVu Sans Mono is a pragmatic first terminal font for the current Witty web distribution:

- monospace and widely used on Linux systems
- covers ASCII, common shell symbols, and broad Unicode ranges better than many
  minimal coding fonts
- redistributable under the Bitstream Vera license plus public-domain DejaVu
  changes
- already compatible with the current browser canvas smoke harness

This is not a final typography decision. Later product work can replace it with
a stronger terminal-focused font after checking glyph coverage, rendering
quality, and license constraints.

## Build Behavior

`scripts/build-witty-web-dist.sh` uses the bundled font by default and copies the
license metadata into:

```text
target/witty-web-dist/licenses/fonts.txt
```

Override builds can still set:

```text
WITTY_WEB_FONT=/path/to/font.ttf
WITTY_WEB_FONT_LICENSE=/path/to/license.txt
```

If an override font is used without `WITTY_WEB_FONT_LICENSE`, the build
emits local-source metadata but marks the output as unsuitable for release
packaging until license metadata is supplied.
