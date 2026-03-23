"""
Callback manager for ghostty runtime.

Provides Python callback implementations for ghostty runtime config.
"""

import logging
from typing import Optional, Any
from weakref import WeakKeyDictionary

from .bindings import ffi, lib

logger = logging.getLogger(__name__)


class GhosttyCallbackManager:
    """
    Manages callbacks for ghostty runtime.

    This class provides Python implementations of ghostty C callbacks
    and handles the interaction with PySide6 event loop.
    """

    def __init__(self, app_instance: Any = None):
        """
        Initialize callback manager.

        Args:
            app_instance: The GhosttyApp instance (for callbacks).
        """
        self.app_instance = app_instance
        self._surfaces = WeakKeyDictionary()

        # Create cffi callbacks with simplified signatures
        # We use void* for complex structures to avoid issues

        self._wakeup_cb = ffi.callback(
            "void(void*)",
            self._wakeup_callback
        )

        # Note: For action_cb, we need to use the correct signature
        # but we'll handle it as void* for now
        # TODO: Define proper structures if needed

        self._read_clipboard_cb = ffi.callback(
            "bool(void*, int, void*)",
            self._read_clipboard_callback
        )

        self._write_clipboard_cb = ffi.callback(
            "void(void*, int, void*, size_t, bool)",
            self._write_clipboard_callback
        )

        self._close_surface_cb = ffi.callback(
            "void(void*, bool)",
            self._close_surface_callback
        )

        # Action callback needs special handling
        # For now, create a simple wrapper that returns False
        self._action_cb = self._create_action_callback()

    def _create_action_callback(self):
        """Create action callback with proper signature."""
        # Action callback has complex signature that we'll simplify
        # Returns False (not handled) for all actions
        @ffi.callback("bool(void*, void*, void*)")
        def action_cb(app, target, action):
            logger.debug("action_cb called (not handled)")
            return False

        return action_cb

    def get_runtime_config(self) -> Any:
        """
        Create and return a ghostty_runtime_config_s structure.

        Returns:
            The runtime config structure with all callbacks set.
        """
        config = ffi.new("ghostty_runtime_config_s *")
        config.userdata = ffi.new_handle(self)
        config.supports_selection_clipboard = True
        config.wakeup_cb = self._wakeup_cb
        config.action_cb = self._action_cb
        config.read_clipboard_cb = self._read_clipboard_cb
        config.write_clipboard_cb = self._write_clipboard_cb
        config.close_surface_cb = self._close_surface_cb

        return config

    # ------------------------------------------------------------------------
    # Callback Implementations
    # ------------------------------------------------------------------------

    def _wakeup_callback(self, userdata: Any) -> None:
        """
        Wakeup callback - called when ghostty needs attention.

        This should wake up the event loop to process ghostty events.
        """
        logger.debug("wakeup_cb called")

        # If we have a PySide6 app, we can use a timer to call tick
        try:
            from PySide6.QtCore import QTimer, QCoreApplication

            app = QCoreApplication.instance()
            if app is not None:
                # Post an event to wake up the event loop
                # This will cause ghostty_app_tick to be called
                if self.app_instance and hasattr(self.app_instance, 'tick'):
                    QTimer.singleShot(0, self.app_instance.tick)

        except ImportError:
            logger.debug("PySide6 not available, cannot wake up event loop")

    def _read_clipboard_callback(self, userdata: Any, clipboard_type: int, state: Any) -> bool:
        """
        Read clipboard callback - called when ghostty wants to read clipboard.

        Args:
            userdata: User data pointer
            clipboard_type: GHOSTTY_CLIPBOARD_STANDARD or GHOSTTY_CLIPBOARD_SELECTION
            state: Opaque state to pass back when clipboard is ready

        Returns:
            True if clipboard read was initiated, False otherwise.
        """
        logger.debug(f"read_clipboard_cb called, type={clipboard_type}")

        try:
            from PySide6.QtGui import QClipboard, QGuiApplication

            clipboard = QGuiApplication.clipboard()
            text = clipboard.text()

            # Call ghostty back with the clipboard content
            if self.app_instance and hasattr(self.app_instance, 'handle_clipboard_read'):
                self.app_instance.handle_clipboard_read(text, state)

            return True

        except Exception as e:
            logger.error(f"Error reading clipboard: {e}")
            return False

    def _write_clipboard_callback(
        self,
        userdata: Any,
        clipboard_type: int,
        content_ptr: Any,
        content_len: int,
        confirm: bool
    ) -> None:
        """
        Write clipboard callback - called when ghostty wants to write to clipboard.

        Args:
            userdata: User data pointer
            clipboard_type: GHOSTTY_CLIPBOARD_STANDARD or GHOSTTY_CLIPBOARD_SELECTION
            content_ptr: Pointer to clipboard content
            content_len: Length of content array
            confirm: Whether to confirm with user
        """
        logger.debug(f"write_clipboard_cb called, type={clipboard_type}")

        try:
            from PySide6.QtGui import QClipboard, QGuiApplication

            # Read the content from ghostty
            # Note: content is an array of ghostty_clipboard_content_s
            if content_ptr != ffi.NULL and content_len > 0:
                # Get first content item
                content = ffi.cast("ghostty_clipboard_content_s*", content_ptr)

                if content.data != ffi.NULL:
                    # Assuming text data
                    text = ffi.string(content.data).decode('utf-8')

                    clipboard = QGuiApplication.clipboard()
                    clipboard.setText(text)

                    logger.debug(f"  Wrote {len(text)} characters to clipboard")

        except Exception as e:
            logger.error(f"Error writing clipboard: {e}")

    def _close_surface_callback(self, userdata: Any, process_alive: bool) -> None:
        """
        Close surface callback - called when a surface should be closed.

        Args:
            userdata: User data pointer
            process_alive: Whether the child process is still alive
        """
        logger.debug(f"close_surface_cb called, process_alive={process_alive}")

        # TODO: Implement surface closing
        # This should remove the surface from the UI

    # ------------------------------------------------------------------------
    # Resource Management
    # ------------------------------------------------------------------------

    def register_surface(self, surface: Any) -> None:
        """Register a surface for management."""
        self._surfaces[surface] = True

    def unregister_surface(self, surface: Any) -> None:
        """Unregister a surface."""
        if surface in self._surfaces:
            del self._surfaces[surface]

    def cleanup(self) -> None:
        """Clean up callback resources."""
        self._surfaces.clear()
        logger.debug("Callback manager cleaned up")