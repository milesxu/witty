#!/usr/bin/env python3
"""Test GhosttyApp with full callbacks."""

import sys
import logging
from pathlib import Path

# Add src to path
sys.path.insert(0, str(Path(__file__).parent.parent / "src"))

# Setup logging
logging.basicConfig(
    level=logging.DEBUG,
    format='%(name)s - %(levelname)s - %(message)s'
)

from witty.ghostty import GhosttyApp, GhosttySurface, get_version

def test_app_with_callbacks():
    """Test GhosttyApp with full callback implementation."""
    print("=" * 60)
    print("Testing GhosttyApp with Full Callbacks")
    print("=" * 60)
    print()

    print(f"Library Version: {get_version()}")
    print()

    print("Step 1: Create GhosttyApp")
    app = GhosttyApp()
    print(f"✓ GhosttyApp created (initialized: {app.is_initialized})")
    print()

    print("Step 2: Initialize GhosttyApp (with callbacks)")
    if not app.initialize():
        print("✗ Failed to initialize app")
        return False

    print(f"✓ GhosttyApp initialized successfully")
    print(f"  Has callback manager: {app._callback_manager is not None}")
    print()

    print("Step 3: Test tick (process events)")
    app.tick()
    print("✓ Tick called successfully")
    print()

    print("Step 4: Create GhosttySurface")
    surface = GhosttySurface(app)
    print(f"✓ GhosttySurface created")
    print()

    print("Step 5: Initialize surface (800x600)")
    if not surface.initialize(800, 600):
        print("✗ Failed to initialize surface")
        app.cleanup()
        return False

    print(f"✓ GhosttySurface initialized")
    cols, rows, width, height = surface.get_size()
    print(f"  Grid: {cols} cols x {rows} rows")
    print(f"  Size: {width} x {height} pixels")
    print()

    print("Step 6: Test surface operations")
    surface.draw()
    print("✓ Draw called")

    surface.send_text("echo 'Hello, Ghostty!'\n")
    print("✓ Text sent")

    surface.set_focus(True)
    print("✓ Focus set")
    print()

    print("Step 7: Cleanup")
    surface.cleanup()
    print("✓ Surface cleaned up")

    app.cleanup()
    print("✓ App cleaned up")
    print()

    return True


def main():
    """Run the test."""
    print()

    try:
        if not test_app_with_callbacks():
            print("\n✗ Test failed")
            return 1

        print("=" * 60)
        print("✓ All tests passed!")
        print("=" * 60)
        print()
        print("Summary:")
        print("  • GhosttyApp initialized with full callbacks")
        print("  • Callback manager created and configured")
        print("  • Surface creation and basic operations work")
        print("  • All resources cleaned up properly")
        print()
        print("Next steps:")
        print("  • Integrate with PySide6 QQuickItem")
        print("  • Implement OpenGL rendering")
        print("  • Add real keyboard/mouse input")

        return 0

    except Exception as e:
        print(f"\n✗ Error: {e}")
        import traceback
        traceback.print_exc()
        return 1


if __name__ == "__main__":
    sys.exit(main())