#!/usr/bin/env python3
"""Minimal test - just load library and call ghostty_info."""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent.parent / "src"))

from witty.ghostty.bindings import ffi, lib, get_version, get_build_mode

print("=" * 60)
print("Minimal Test: Library Load + ghostty_info()")
print("=" * 60)
print()

if lib is None:
    print("✗ Library not loaded")
    sys.exit(1)

print("✓ Library loaded")
print()

# Test basic info
try:
    version = get_version()
    print(f"Version: {version}")

    build_mode = get_build_mode()
    print(f"Build Mode: {build_mode}")
    print()

    print("=" * 60)
    print("✓ SUCCESS!")
    print("=" * 60)
except Exception as e:
    print(f"✗ Error: {e}")
    import traceback
    traceback.print_exc()
    sys.exit(1)