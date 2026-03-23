"""GhosttyApp wrapper class for managing ghostty application instance."""

import logging
from typing import Optional

from .bindings import ffi, lib
from .callbacks import GhosttyCallbackManager

logger = logging.getLogger(__name__)


class GhosttyApp:
    """
    Python wrapper for ghostty_app_t.

    Manages the ghostty application instance and its lifecycle.
    """

    def __init__(self):
        """Initialize GhosttyApp."""
        self._app = None
        self._config = None
        self._initialized = False
        self._callback_manager = None
        self._runtime_config = None

    def initialize(self) -> bool:
        """
        Initialize the ghostty application.

        Returns:
            True if initialization successful, False otherwise.
        """
        if self._initialized:
            logger.warning("GhosttyApp already initialized")
            return True

        if lib is None:
            logger.error("libghostty not loaded")
            return False

        try:
            # Initialize ghostty library
            result = lib.ghostty_init(0, ffi.NULL)
            if result != 0:
                logger.error(f"ghostty_init failed with code {result}")
                return False

            # Create config
            self._config = lib.ghostty_config_new()
            if self._config == ffi.NULL:
                logger.error("Failed to create config")
                return False

            # Finalize config (required before creating app)
            lib.ghostty_config_finalize(self._config)

            # Create callback manager
            self._callback_manager = GhosttyCallbackManager(self)

            # Get runtime config with callbacks
            self._runtime_config = self._callback_manager.get_runtime_config()

            # Create app with runtime config
            self._app = lib.ghostty_app_new(self._runtime_config, self._config)
            if self._app == ffi.NULL:
                logger.error("Failed to create app")
                lib.ghostty_config_free(self._config)
                self._config = None
                self._callback_manager.cleanup()
                self._callback_manager = None
                return False

            self._initialized = True
            logger.info("✓ GhosttyApp initialized successfully")
            return True

        except Exception as e:
            logger.error(f"Error initializing GhosttyApp: {e}")
            import traceback
            traceback.print_exc()
            return False

    def tick(self) -> None:
        """
        Process ghostty events.

        Should be called regularly to process ghostty internal events.
        """
        if not self._initialized or self._app is None:
            return

        lib.ghostty_app_tick(self._app)

    def set_focus(self, focused: bool) -> None:
        """
        Set application focus state.

        Args:
            focused: Whether the application is focused.
        """
        if not self._initialized or self._app is None:
            return

        lib.ghostty_app_set_focus(self._app, focused)

    def handle_clipboard_read(self, text: str, state: Any) -> None:
        """
        Handle clipboard read result.

        Args:
            text: The clipboard text.
            state: Opaque state from the callback.
        """
        # TODO: Implement clipboard handling
        # This will be called by the callback manager
        logger.debug(f"Clipboard read: {len(text)} characters")

    @property
    def handle(self):
        """Get the underlying ghostty_app_t handle."""
        return self._app

    @property
    def config_handle(self):
        """Get the underlying ghostty_config_t handle."""
        return self._config

    @property
    def is_initialized(self) -> bool:
        """Check if the app is initialized."""
        return self._initialized

    def __del__(self):
        """Clean up ghostty resources."""
        self.cleanup()

    def cleanup(self) -> None:
        """Explicitly clean up resources."""
        if self._app is not None and self._app != ffi.NULL:
            lib.ghostty_app_free(self._app)
            self._app = None

        if self._config is not None and self._config != ffi.NULL:
            lib.ghostty_config_free(self._config)
            self._config = None

        if self._callback_manager is not None:
            self._callback_manager.cleanup()
            self._callback_manager = None

        self._initialized = False
        self._runtime_config = None
        logger.info("✓ GhosttyApp cleaned up")