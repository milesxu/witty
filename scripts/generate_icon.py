#!/usr/bin/env python3
"""Generate application icons from SVG source."""

from pathlib import Path

import cairosvg
from PIL import Image


def generate_icons() -> None:
    """Generate PNG icons in various sizes from SVG."""
    # Paths
    assets_dir = Path(__file__).parent.parent / "assets"
    svg_path = assets_dir / "icon.svg"
    icons_dir = assets_dir / "icons"
    icons_dir.mkdir(exist_ok=True)

    # Icon sizes for different purposes
    sizes = [16, 24, 32, 48, 64, 128, 256, 512]

    print(f"Generating icons from {svg_path}...")

    for size in sizes:
        output_path = icons_dir / f"icon_{size}x{size}.png"

        # Convert SVG to PNG using cairosvg
        cairosvg.svg2png(
            url=str(svg_path),
            write_to=str(output_path),
            output_width=size,
            output_height=size,
        )

        print(f"  ✓ Generated {output_path.name}")

    # Also create a base icon.png (256x256 for general use)
    icon_path = assets_dir / "icon.png"
    cairosvg.svg2png(
        url=str(svg_path),
        write_to=str(icon_path),
        output_width=256,
        output_height=256,
    )
    print(f"  ✓ Generated {icon_path.name}")

    print("\n✅ All icons generated successfully!")
    print(f"   Location: {icons_dir}")


if __name__ == "__main__":
    generate_icons()