#!/usr/bin/env python3
"""Test TerminalWidget with actual PySide6 window."""

import sys
import logging
from pathlib import Path

# Add src to path
sys.path.insert(0, str(Path(__file__).parent.parent / "src"))

# Setup logging
logging.basicConfig(
    level=logging.INFO,
    format='%(name)s - %(levelname)s - %(message)s'
)

from PySide6.QtWidgets import QApplication, QMainWindow, QVBoxLayout, QWidget
from PySide6.QtCore import Qt

# Import our terminal widget
from witty.gui.terminal_widget import TerminalWidget

logger = logging.getLogger(__name__)


class MainWindow(QMainWindow):
    """Main window containing the terminal widget."""

    def __init__(self):
        """Initialize the main window."""
        super().__init__()

        self.setWindowTitle("Witty Terminal - PySide6 Test")
        self.setGeometry(100, 100, 800, 600)

        # Create central widget
        central = QWidget()
        self.setCentralWidget(central)

        # Create layout
        layout = QVBoxLayout(central)
        layout.setContentsMargins(0, 0, 0, 0)

        # Create terminal widget
        self.terminal = TerminalWidget()
        layout.addWidget(self.terminal)

        logger.info("MainWindow created")

    def showEvent(self, event):
        """Handle show event - initialize terminal after widget is shown."""
        super().showEvent(event)

        # Initialize terminal after window is shown
        if not self.terminal._app:
            logger.info("Window shown, initializing terminal...")
            if self.terminal.initialize():
                logger.info("✓ Terminal initialized successfully!")
            else:
                logger.error("✗ Terminal initialization failed")

    def closeEvent(self, event):
        """Handle close event."""
        logger.info("Closing main window...")
        self.terminal.cleanup()
        super().closeEvent(event)


def main():
    """Run the test application."""
    logger.info("=" * 60)
    logger.info("Testing TerminalWidget with PySide6 Window")
    logger.info("=" * 60)

    # Create application
    app = QApplication(sys.argv)

    # Set application info
    app.setApplicationName("Witty Terminal")
    app.setOrganizationName("Witty")

    # Create main window
    window = MainWindow()
    window.show()

    logger.info("Window shown, entering event loop...")

    # Run event loop
    result = app.exec()

    logger.info("Application exited with code %d", result)
    logger.info("=" * 60)

    return result


if __name__ == "__main__":
    sys.exit(main())