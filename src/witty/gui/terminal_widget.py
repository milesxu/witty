"""Terminal widget using PySide6 and ghostty."""

import logging
from typing import Optional

from PySide6.QtCore import Qt, QTimer
from PySide6.QtWidgets import QWidget
from PySide6.QtGui import QPaintEvent, QResizeEvent

from ..ghostty import GhosttyApp, GhosttySurface

logger = logging.getLogger(__name__)


class TerminalWidget(QWidget):
    """
    A QWidget that hosts a ghostty terminal surface.

    This widget creates and manages a ghostty terminal surface,
    handling rendering, input, and lifecycle.
    """

    def __init__(self, parent=None):
        """Initialize the terminal widget."""
        super().__init__(parent)

        # Ghostty components
        self._app: Optional[GhosttyApp] = None
        self._surface: Optional[GhosttySurface] = None

        # Widget setup
        self.setMinimumSize(400, 300)
        self.setFocusPolicy(Qt.StrongFocus)
        self.setAttribute(Qt.WA_OpaquePaintEvent)
        self.setAttribute(Qt.WA_NoSystemBackground)

        # Timer for regular tick calls
        self._tick_timer = QTimer(self)
        self._tick_timer.timeout.connect(self._on_tick)
        self._tick_timer.setInterval(16)  # ~60 FPS

        logger.debug("TerminalWidget created")

    def initialize(self) -> bool:
        """
        Initialize the ghostty app and surface.

        Returns:
            True if initialization successful, False otherwise.
        """
        logger.info("Initializing TerminalWidget...")

        # Create and initialize ghostty app
        self._app = GhosttyApp()
        if not self._app.initialize():
            logger.error("Failed to initialize GhosttyApp")
            return False

        logger.info("✓ GhosttyApp initialized")

        # Create surface
        self._surface = GhosttySurface(self._app)

        # Try to create surface with window ID
        width = self.width()
        height = self.height()

        logger.info(f"Creating surface with size {width}x{height}")
        logger.info(f"Window ID: {self.winId()}")

        # Try different platform data options
        success = False

        # Option 1: Use window ID
        win_id = int(self.winId())
        logger.info(f"Trying with window ID: {win_id}")

        from ..ghostty.bindings import ffi
        platform_data = ffi.cast("void*", win_id)

        if not self._surface.initialize(width, height, platform_data):
            logger.warning("Failed with window ID, trying NULL...")

            # Option 2: Use NULL
            if not self._surface.initialize(width, height, ffi.NULL):
                logger.error("Failed to create surface with all options")
                return False

        logger.info("✓ GhosttySurface created")
        success = True

        # Start tick timer
        self._tick_timer.start()

        logger.info("✓ TerminalWidget initialized successfully")
        return success

    def _on_tick(self):
        """Process ghostty events."""
        if self._app:
            self._app.tick()

    def paintEvent(self, event: QPaintEvent):
        """Handle paint event."""
        if self._surface and self._surface.is_initialized:
            # Trigger ghostty rendering
            self._surface.draw()

    def resizeEvent(self, event: QResizeEvent):
        """Handle resize event."""
        if self._surface and self._surface.is_initialized:
            width = event.size().width()
            height = event.size().height()
            self._surface.set_size(width, height)
            logger.debug(f"Surface resized to {width}x{height}")

    def keyPressEvent(self, event):
        """Handle key press."""
        if self._surface and self._surface.is_initialized:
            # TODO: Convert Qt key event to ghostty key event
            # For now, send as text
            text = event.text()
            if text:
                self._surface.send_text(text)

    def focusInEvent(self, event):
        """Handle focus in."""
        if self._surface:
            self._surface.set_focus(True)

    def focusOutEvent(self, event):
        """Handle focus out."""
        if self._surface:
            self._surface.set_focus(False)

    def cleanup(self):
        """Clean up resources."""
        logger.info("Cleaning up TerminalWidget...")

        self._tick_timer.stop()

        if self._surface:
            self._surface.cleanup()
            self._surface = None

        if self._app:
            self._app.cleanup()
            self._app = None

        logger.info("✓ TerminalWidget cleaned up")

    def closeEvent(self, event):
        """Handle close event."""
        self.cleanup()
        event.accept()

    def __del__(self):
        """Destructor."""
        self.cleanup()