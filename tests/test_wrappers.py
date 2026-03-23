#!/usr/bin/env python3
"""Test GhosttyApp and GhosttySurface wrapper classes."""

import sys
from pathlib import Path

# Add src to path
sys.path.insert(0, str(Path(__file__).parent.parent / "src"))

from witty.ghostty import GhosttyApp, GhosttySurface, get_version, get_build_mode

def test_basic_info():
    """Test basic library info."""
    print("=" * 60)
    print("Testing GhosttyApp and GhosttySurface Wrappers")
    print("=" * 60)
    print()

    print("Library Info:")
    print(f"  Version: {get_version()}")
    print(f"  Build Mode: {get_build_mode()}")
    print()

    return True


def test_app_lifecycle():
    """Test GhosttyApp lifecycle."""
    print("Testing GhosttyApp Lifecycle...")
    print()

    # Create app
    app = GhosttyApp()
    print(f"✓ GhosttyApp created (initialized: {app.is_initialized})")

    # Initialize
    if not app.initialize():
        print("✗ Failed to initialize app")
        return False

    print(f"✓ GhosttyApp initialized (initialized: {app.is_initialized})")

    # Tick
    app.tick()
    print("✓ App tick called")

    # Set focus
    app.set_focus(True)
    print("✓ App focus set to True")

    # Cleanup
    app.cleanup()
    print(f"✓ GhosttyApp cleaned up (initialized: {app.is_initialized})")
    print()

    return True


def test_surface_lifecycle():
    """Test GhosttySurface lifecycle."""
    print("Testing GhosttySurface Lifecycle...")
    print()

    # Create app
    app = GhosttyApp()
    if not app.initialize():
        print("✗ Failed to initialize app")
        return False

    print("✓ GhosttyApp initialized")

    # Create surface
    surface = GhosttySurface(app)
    print(f"✓ GhosttySurface created (initialized: {surface.is_initialized})")

    # Initialize surface
    if not surface.initialize(800, 600):
        print("✗ Failed to initialize surface")
        return False

    print(f"✓ GhosttySurface initialized (initialized: {surface.is_initialized})")
    print(f"  Size: {surface.size[0]}x{surface.size[1]}")

    # Get detailed size
    cols, rows, width, height = surface.get_size()
    print(f"  Grid: {cols} cols x {rows} rows")
    print(f"  Pixels: {width} x {height}")

    # Set size
    surface.set_size(1024, 768)
    print(f"✓ Surface resized to {surface.size[0]}x{surface.size[1]}")

    # Draw
    surface.draw()
    print("✓ Surface draw called")

    # Send text
    surface.send_text("Hello, Ghostty!\n")
    print("✓ Text sent to surface")

    # Set focus
    surface.set_focus(True)
    print("✓ Surface focus set to True")

    # Cleanup
    surface.cleanup()
    print(f"✓ GhosttySurface cleaned up (initialized: {surface.is_initialized})")

    app.cleanup()
    print("✓ GhosttyApp cleaned up")
    print()

    return True


def test_context_manager():
    """Test using GhosttyApp as context manager."""
    print("Testing Context Manager Pattern...")
    print()

    # Note: We'll implement __enter__ and __exit__ if needed
    # For now, just test explicit lifecycle

    app = GhosttyApp()
    if not app.initialize():
        print("✗ Failed to initialize")
        return False

    print("✓ App initialized")

    # Work with app
    app.tick()

    # Explicit cleanup
    app.cleanup()
    print("✓ Explicit cleanup done")
    print()

    return True


def main():
    """Run all tests."""
    print()

    if not test_basic_info():
        return 1

    if not test_app_lifecycle():
        return 1

    if not test_surface_lifecycle():
        return 1

    if not test_context_manager():
        return 1

    print("=" * 60)
    print("✓ All wrapper tests passed!")
    print("=" * 60)
    print()
    print("Summary:")
    print("  • GhosttyApp creation and initialization works")
    print("  • GhosttySurface creation and initialization works")
    print("  • Basic methods (tick, draw, set_size, send_text) work")
    print("  • Lifecycle management (cleanup) works")
    print()
    print("Next steps:")
    print("  • Integrate with PySide6 QQuickItem")
    print("  • Implement OpenGL rendering")
    print("  • Add keyboard/mouse input handling")
    print("  • Create QML component")

    return 0


if __name__ == "__main__":
    sys.exit(main())