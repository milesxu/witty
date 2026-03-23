#!/usr/bin/env python3
"""Test script to verify libghostty can be loaded and basic API works."""

import sys
from pathlib import Path

# Add src to path
sys.path.insert(0, str(Path(__file__).parent.parent / "src"))

from witty.ghostty.bindings import ffi, lib

def test_ghostty_info():
    """Test ghostty_info() function."""
    print("Testing ghostty_info()...")
    try:
        info = lib.ghostty_info()

        version_str = ffi.string(info.version).decode('utf-8')
        print(f"  Version: {version_str}")
        print(f"  Major: {info.version_major}")
        print(f"  Minor: {info.version_minor}")
        print(f"  Patch: {info.version_patch}")
        print("✓ ghostty_info() works!\n")
        return True
    except Exception as e:
        print(f"✗ Error: {e}\n")
        return False

def main():
    """Run basic tests."""
    print("=" * 60)
    print("libghostty Binding Test - Minimal")
    print("=" * 60 + "\n")

    try:
        if test_ghostty_info():
            print("=" * 60)
            print("✓ Library loaded successfully!")
            print("=" * 60)
            return 0
        else:
            print("✗ Tests failed")
            return 1

    except Exception as e:
        print(f"\n✗ Error: {e}")
        import traceback
        traceback.print_exc()
        return 1

if __name__ == "__main__":
    sys.exit(main())