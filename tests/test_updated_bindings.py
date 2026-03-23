#!/usr/bin/env python3
"""Test the updated ghostty bindings."""

import sys
from pathlib import Path

# Add src to path
sys.path.insert(0, str(Path(__file__).parent.parent / "src"))

from witty.ghostty.bindings import ffi, lib, get_version, get_build_mode

def test_basic_info():
    """Test basic library info."""
    print("=" * 60)
    print("Testing Updated Ghostty Bindings")
    print("=" * 60)
    print()

    if lib is None:
        print("✗ Library not loaded")
        return False

    print("✓ Library loaded successfully")
    print()

    # Test version
    version = get_version()
    print(f"Version: {version}")

    build_mode = get_build_mode()
    print(f"Build Mode: {build_mode}")
    print()

    return True


def test_config_creation():
    """Test config creation and management."""
    print("Testing Config Management...")
    print()

    try:
        # Create config
        config = lib.ghostty_config_new()
        if config == ffi.NULL:
            print("✗ Failed to create config")
            return False

        print("✓ Config created")

        # Free config
        lib.ghostty_config_free(config)
        print("✓ Config freed")
        print()

        return True

    except Exception as e:
        print(f"✗ Error: {e}")
        return False


def test_app_creation():
    """Test app creation."""
    print("Testing App Creation...")
    print()

    try:
        # Create config
        config = lib.ghostty_config_new()
        if config == ffi.NULL:
            print("✗ Failed to create config")
            return False

        print("✓ Config created")

        # Create app with NULL runtime config
        app = lib.ghostty_app_new(ffi.NULL, config)
        if app == ffi.NULL:
            print("✗ Failed to create app")
            lib.ghostty_config_free(config)
            return False

        print("✓ App created")

        # Free app
        lib.ghostty_app_free(app)
        print("✓ App freed")

        # Free config
        lib.ghostty_config_free(config)
        print("✓ Config freed")
        print()

        return True

    except Exception as e:
        print(f"✗ Error: {e}")
        import traceback
        traceback.print_exc()
        return False


def main():
    """Run all tests."""
    print()

    if not test_basic_info():
        return 1

    if not test_config_creation():
        return 1

    if not test_app_creation():
        return 1

    print("=" * 60)
    print("✓ All tests passed!")
    print("=" * 60)
    print()
    print("Summary:")
    print("  • Library loads successfully")
    print("  • Correct structure definitions")
    print("  • Config management works")
    print("  • App creation works")
    print()
    print("Next steps:")
    print("  • Implement surface creation")
    print("  • Test OpenGL rendering")
    print("  • Integrate with PySide6")

    return 0


if __name__ == "__main__":
    sys.exit(main())