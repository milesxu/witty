"""QML-based GUI module for Witty Terminal."""

import sys
from pathlib import Path

from PySide6.QtWidgets import QApplication
from PySide6.QtGui import QIcon
from PySide6.QtQml import QQmlApplicationEngine


def run_gui() -> None:
    """Run the QML-based GUI application."""
    # Use QApplication instead of QGuiApplication for better icon support
    app = QApplication(sys.argv)

    # Set application icon with multiple sizes
    assets_dir = Path(__file__).parent.parent.parent.parent / "assets"
    icons_dir = assets_dir / "icons"

    icon = QIcon()

    # Add multiple icon sizes for better display quality
    icon_sizes = [16, 24, 32, 48, 64, 128, 256]

    for size in icon_sizes:
        icon_path = icons_dir / f"icon_{size}x{size}.png"
        if icon_path.exists():
            icon.addFile(str(icon_path))

    # Fallback to main icon
    main_icon = assets_dir / "icon.png"
    if main_icon.exists():
        icon.addFile(str(main_icon))

    app.setWindowIcon(icon)
    print(f"✓ Icon loaded with {len(icon.availableSizes())} sizes")

    # Set application name and organization
    app.setApplicationName("Witty Terminal")
    app.setOrganizationName("Witty")

    engine = QQmlApplicationEngine()
    qml_file = Path(__file__).parent / "main.qml"

    engine.load(qml_file)

    if not engine.rootObjects():
        sys.exit(-1)

    sys.exit(app.exec())


if __name__ == "__main__":
    run_gui()