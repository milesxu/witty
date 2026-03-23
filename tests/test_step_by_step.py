#!/usr/bin/env python3
"""Step-by-step minimal test for ghostty API calls."""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent.parent / "src"))

from witty.ghostty.bindings import ffi, lib

def main():
    print("=" * 60)
    print("Step-by-Step Minimal Test")
    print("=" * 60)
    print()

    if lib is None:
        print("✗ Library not loaded")
        return 1

    print("Step 1: Load library")
    print("✓ Library loaded")
    print()

    print("Step 2: Call ghostty_info()")
    info = lib.ghostty_info()
    version = ffi.string(info.version, info.version_len).decode('utf-8')
    print(f"✓ Version: {version}")
    print()

    print("Step 3: Create config")
    config = lib.ghostty_config_new()
    if config == ffi.NULL:
        print("✗ Failed to create config")
        return 1
    print("✓ Config created")
    print()

    # Skip ghostty_init and config_finalize for now
    print("Step 4: Create app (without init)")
    app = lib.ghostty_app_new(ffi.NULL, config)
    if app == ffi.NULL:
        print("✗ Failed to create app")
        lib.ghostty_config_free(config)
        return 1
    print("✓ App created")
    print()

    print("Step 5: Free resources")
    lib.ghostty_app_free(app)
    print("✓ App freed")
    lib.ghostty_config_free(config)
    print("✓ Config freed")
    print()

    print("=" * 60)
    print("✓ Minimal test passed!")
    print("=" * 60)

    return 0


if __name__ == "__main__":
    sys.exit(main())