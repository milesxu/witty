"""
Ghostty C API bindings using cffi.

This file provides Python bindings for libghostty using cffi.
All structure definitions are copied exactly from /usr/local/include/ghostty.h
"""

from pathlib import Path
from typing import Optional

from cffi import FFI

# Initialize FFI
ffi = FFI()

# ============================================================================
# Core Type Definitions (from ghostty.h)
# Note: cffi provides built-in types: bool, uint8_t, uint32_t, etc.
# ============================================================================

ffi.cdef("""
    // ----------------------------------------------------------------
    // Opaque Types
    // ----------------------------------------------------------------

    typedef void* ghostty_app_t;
    typedef void* ghostty_config_t;
    typedef void* ghostty_surface_t;
    typedef void* ghostty_inspector_t;

    // ----------------------------------------------------------------
    // Enums
    // ----------------------------------------------------------------

    typedef enum {
        GHOSTTY_PLATFORM_INVALID,
        GHOSTTY_PLATFORM_MACOS,
        GHOSTTY_PLATFORM_IOS,
    } ghostty_platform_e;

    typedef enum {
        GHOSTTY_CLIPBOARD_STANDARD,
        GHOSTTY_CLIPBOARD_SELECTION,
    } ghostty_clipboard_e;

    typedef enum {
        GHOSTTY_BUILD_MODE_DEBUG,
        GHOSTTY_BUILD_MODE_RELEASE_SAFE,
        GHOSTTY_BUILD_MODE_RELEASE_FAST,
        GHOSTTY_BUILD_MODE_RELEASE_SMALL,
    } ghostty_build_mode_e;

    typedef enum {
        GHOSTTY_COLOR_SCHEME_LIGHT = 0,
        GHOSTTY_COLOR_SCHEME_DARK = 1,
    } ghostty_color_scheme_e;

    typedef enum {
        GHOSTTY_MODS_NONE = 0,
        GHOSTTY_MODS_SHIFT = 1 << 0,
        GHOSTTY_MODS_CTRL = 1 << 1,
        GHOSTTY_MODS_ALT = 1 << 2,
        GHOSTTY_MODS_SUPER = 1 << 3,
        GHOSTTY_MODS_CAPS = 1 << 4,
        GHOSTTY_MODS_NUM = 1 << 5,
    } ghostty_input_mods_e;

    typedef enum {
        GHOSTTY_ACTION_RELEASE,
        GHOSTTY_ACTION_PRESS,
        GHOSTTY_ACTION_REPEAT,
    } ghostty_input_action_e;

    typedef enum {
        GHOSTTY_MOUSE_RELEASE,
        GHOSTTY_MOUSE_PRESS,
    } ghostty_input_mouse_state_e;

    typedef enum {
        GHOSTTY_MOUSE_UNKNOWN,
        GHOSTTY_MOUSE_LEFT,
        GHOSTTY_MOUSE_RIGHT,
        GHOSTTY_MOUSE_MIDDLE,
    } ghostty_input_mouse_button_e;

    // ----------------------------------------------------------------
    // Structures
    // ----------------------------------------------------------------

    typedef struct {
        ghostty_build_mode_e build_mode;
        const char* version;
        uintptr_t version_len;
    } ghostty_info_s;

    typedef struct {
        const char* message;
    } ghostty_diagnostic_s;

    typedef struct {
        const char* ptr;
        uintptr_t len;
        bool sentinel;
    } ghostty_string_s;

    typedef struct {
        ghostty_input_action_e action;
        ghostty_input_mods_e mods;
        uint32_t keycode;
        char utf8[4];
        uintptr_t utf8_len;
        bool consumed;
    } ghostty_input_key_s;

    typedef struct {
        uint32_t columns;
        uint32_t rows;
        uint32_t width;
        uint32_t height;
    } ghostty_surface_size_s;

    typedef struct {
        ghostty_clipboard_e clipboard;
        int request;  // ghostty_clipboard_request_e
        void* userdata;
    } ghostty_clipboard_s;

    typedef struct {
        const char* mime;
        const char* data;
    } ghostty_clipboard_content_s;

    // ----------------------------------------------------------------
    // Callback Types
    // ----------------------------------------------------------------

    typedef void (*ghostty_runtime_wakeup_cb)(void* userdata);
    typedef bool (*ghostty_runtime_action_cb)(void* app, void* target, void* action);
    typedef bool (*ghostty_runtime_read_clipboard_cb)(void* userdata, int clipboard, void* state);
    typedef void (*ghostty_runtime_write_clipboard_cb)(void* userdata, int clipboard, void* content, size_t len, bool confirm);
    typedef void (*ghostty_runtime_close_surface_cb)(void* userdata, bool process_alive);

    typedef struct {
        void* userdata;
        bool supports_selection_clipboard;
        ghostty_runtime_wakeup_cb wakeup_cb;
        ghostty_runtime_action_cb action_cb;
        ghostty_runtime_read_clipboard_cb read_clipboard_cb;
        void* confirm_read_clipboard_cb;  // ghostty_runtime_confirm_read_clipboard_cb
        ghostty_runtime_write_clipboard_cb write_clipboard_cb;
        ghostty_runtime_close_surface_cb close_surface_cb;
    } ghostty_runtime_config_s;

    // ----------------------------------------------------------------
    // Surface Config
    // ----------------------------------------------------------------

    typedef struct {
        uint32_t width;
        uint32_t height;
    } ghostty_surface_config_s;

    // ----------------------------------------------------------------
    // Functions
    // ----------------------------------------------------------------

    // Initialization and info
    int ghostty_init(uintptr_t argc, char** argv);
    ghostty_info_s ghostty_info(void);

    // Config management
    ghostty_config_t ghostty_config_new(void);
    void ghostty_config_free(ghostty_config_t config);
    void ghostty_config_load_file(ghostty_config_t config, const char* path);
    void ghostty_config_finalize(ghostty_config_t config);

    // App management
    ghostty_app_t ghostty_app_new(
        const ghostty_runtime_config_s* runtime_config,
        ghostty_config_t config
    );
    void ghostty_app_free(ghostty_app_t app);
    void ghostty_app_tick(ghostty_app_t app);

    // Surface management
    ghostty_surface_config_s ghostty_surface_config_new(void);
    ghostty_surface_t ghostty_surface_new(
        ghostty_app_t app,
        const ghostty_surface_config_s* config,
        void* platform
    );
    void ghostty_surface_free(ghostty_surface_t surface);
    void ghostty_surface_draw(ghostty_surface_t surface);
    void ghostty_surface_set_size(ghostty_surface_t surface, uint32_t width, uint32_t height);
    ghostty_surface_size_s ghostty_surface_size(ghostty_surface_t surface);

    // Input handling
    bool ghostty_surface_key(ghostty_surface_t surface, ghostty_input_key_s key);
    void ghostty_surface_text(ghostty_surface_t surface, const char* text, uintptr_t len);

    // Utility
    void ghostty_string_free(ghostty_string_s str);
""")

# ============================================================================
# Library Loading
# ============================================================================

_lib = None

def get_lib() -> Optional[object]:
    """
    Get or load the ghostty library.

    Returns:
        The loaded library object, or None if not found.
    """
    global _lib

    if _lib is not None:
        return _lib

    # Try different paths for libghostty.so
    lib_paths = [
        "/home/xuming/src/ghostty/zig-out/lib/libghostty.so",  # Debug build
        "/usr/local/lib/libghostty.so",  # System install
        "/usr/lib/libghostty.so",
        "libghostty.so",  # System library path
    ]

    for path in lib_paths:
        try:
            _lib = ffi.dlopen(path)
            print(f"✓ Loaded libghostty from {path}")
            return _lib
        except OSError:
            continue

    raise RuntimeError(
        "Cannot find libghostty.so. Please ensure libghostty is installed. "
        "Tried paths: " + ", ".join(lib_paths)
    )

# Load the library
try:
    lib = get_lib()
except RuntimeError as e:
    print(f"Warning: {e}")
    lib = None


# ============================================================================
# Convenience Functions
# ============================================================================

def get_version() -> str:
    """Get the ghostty version string."""
    if lib is None:
        raise RuntimeError("libghostty not loaded")

    info = lib.ghostty_info()
    version = ffi.string(info.version, info.version_len).decode('utf-8')
    return version


def get_build_mode() -> str:
    """Get the build mode string."""
    if lib is None:
        raise RuntimeError("libghostty not loaded")

    info = lib.ghostty_info()
    modes = ["Debug", "ReleaseSafe", "ReleaseFast", "ReleaseSmall"]
    return modes[info.build_mode]


__all__ = ['ffi', 'lib', 'get_version', 'get_build_mode']