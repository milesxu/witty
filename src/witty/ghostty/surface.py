"""GhosttySurface wrapper class for managing terminal surface."""

from typing import Optional, Tuple

from .bindings import ffi, lib


class GhosttySurface:
    """
    Python wrapper for ghostty_surface_t.

    Represents a single terminal surface (widget).
    """

    def __init__(self, app):
        """
        Initialize GhosttySurface.

        Args:
            app: GhosttyApp instance.
        """
        self._surface = None
        self._app = app
        self._initialized = False
        self._width = 0
        self._height = 0

    def initialize(self, width: int = 800, height: int = 600, platform_data=None) -> bool:
        """
        Initialize the terminal surface.

        Args:
            width: Initial width in pixels.
            height: Initial height in pixels.
            platform_data: Platform-specific data (e.g., window ID).
                          Can be None, a pointer, or an integer (window ID).

        Returns:
            True if initialization successful, False otherwise.
        """
        if self._initialized:
            print("Warning: GhosttySurface already initialized")
            return True

        if lib is None:
            print("Error: libghostty not loaded")
            return False

        if not self._app.is_initialized:
            print("Error: GhosttyApp not initialized")
            return False

        try:
            # Create surface config
            surface_config = lib.ghostty_surface_config_new()

            # surface_config is returned by value, not pointer
            # We need to copy it to a new allocated struct
            config_ptr = ffi.new("ghostty_surface_config_s *")
            config_ptr[0] = surface_config
            config_ptr.width = width
            config_ptr.height = height

            # Handle platform_data parameter
            if platform_data is None:
                platform_ptr = ffi.NULL
            elif isinstance(platform_data, int):
                # Convert integer (window ID) to void*
                platform_ptr = ffi.cast("void*", platform_data)
            else:
                # Assume it's already a cffi pointer
                platform_ptr = platform_data

            # Create surface
            # The actual library requires 3 parameters:
            # 1. ghostty_app_t
            # 2. surface_config
            # 3. platform_data (window ID or NULL)
            self._surface = lib.ghostty_surface_new(
                self._app.handle,
                config_ptr,
                platform_ptr
            )

            if self._surface == ffi.NULL:
                print("Error: Failed to create surface")
                return False

            self._width = width
            self._height = height
            self._initialized = True

            print(f"✓ GhosttySurface initialized ({width}x{height})")
            return True

        except Exception as e:
            print(f"Error initializing GhosttySurface: {e}")
            import traceback
            traceback.print_exc()
            return False

    def set_size(self, width: int, height: int) -> None:
        """
        Set the surface size.

        Args:
            width: Width in pixels.
            height: Height in pixels.
        """
        if not self._initialized or self._surface is None:
            return

        lib.ghostty_surface_set_size(self._surface, width, height)
        self._width = width
        self._height = height

    def get_size(self) -> Tuple[int, int, int, int]:
        """
        Get the surface size.

        Returns:
            Tuple of (columns, rows, width, height).
        """
        if not self._initialized or self._surface is None:
            return (0, 0, 0, 0)

        size = lib.ghostty_surface_size(self._surface)
        return (size.columns, size.rows, size.width, size.height)

    def draw(self) -> None:
        """Render the surface."""
        if not self._initialized or self._surface is None:
            return

        lib.ghostty_surface_draw(self._surface)

    def set_focus(self, focused: bool) -> None:
        """
        Set surface focus state.

        Args:
            focused: Whether the surface is focused.
        """
        if not self._initialized or self._surface is None:
            return

        lib.ghostty_surface_set_focus(self._surface, focused)

    def send_text(self, text: str) -> None:
        """
        Send text to the terminal.

        Args:
            text: Text to send.
        """
        if not self._initialized or self._surface is None:
            return

        text_bytes = text.encode('utf-8')
        lib.ghostty_surface_text(
            self._surface,
            text_bytes,
            len(text_bytes)
        )

    def send_key(self, keycode: int, mods: int = 0, action: int = 1) -> bool:
        """
        Send a key event to the terminal.

        Args:
            keycode: Key code (ghostty_input_key_e).
            mods: Modifiers (ghostty_input_mods_e).
            action: Action (PRESS=1, RELEASE=0, REPEAT=2).

        Returns:
            True if the key was consumed.
        """
        if not self._initialized or self._surface is None:
            return False

        # Create key event structure
        key = ffi.new("ghostty_input_key_s *")
        key.keycode = keycode
        key.mods = mods
        key.action = action
        key.utf8_len = 0
        key.consumed = False

        result = lib.ghostty_surface_key(self._surface, key[0])
        return result

    @property
    def handle(self):
        """Get the underlying ghostty_surface_t handle."""
        return self._surface

    @property
    def is_initialized(self) -> bool:
        """Check if the surface is initialized."""
        return self._initialized

    @property
    def size(self) -> Tuple[int, int]:
        """Get surface size (width, height)."""
        return (self._width, self._height)

    def __del__(self):
        """Clean up surface resources."""
        self.cleanup()

    def cleanup(self) -> None:
        """Explicitly clean up resources."""
        if self._surface is not None and self._surface != ffi.NULL:
            lib.ghostty_surface_free(self._surface)
            self._surface = None

        self._initialized = False
        print("✓ GhosttySurface cleaned up")