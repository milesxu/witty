#!/usr/bin/env python3
"""Test loading libghostty with CORRECT structure definitions."""

import sys
from cffi import FFI

print("=" * 60)
print("Testing libghostty with CORRECT bindings")
print("=" * 60)
print()

# Initialize FFI with correct structure definitions
ffi = FFI()

# Define the build mode enum first
ffi.cdef("""
    typedef enum {
        GHOSTTY_BUILD_MODE_DEBUG,
        GHOSTTY_BUILD_MODE_RELEASE_SAFE,
        GHOSTTY_BUILD_MODE_RELEASE_FAST,
        GHOSTTY_BUILD_MODE_RELEASE_SMALL,
    } ghostty_build_mode_e;

    typedef struct {
        ghostty_build_mode_e build_mode;
        const char* version;
        uintptr_t version_len;
    } ghostty_info_s;

    ghostty_info_s ghostty_info(void);
""")

# Load library
lib_path = "/home/xuming/src/ghostty/zig-out/lib/libghostty.so"

print(f"Loading: {lib_path}")
print()

try:
    lib = ffi.dlopen(lib_path)
    print("✓ Library loaded successfully!")
    print()

    print("Testing ghostty_info()...")
    info = lib.ghostty_info()

    # Correctly access the fields
    build_mode = info.build_mode
    version = ffi.string(info.version, info.version_len).decode('utf-8')

    print(f"  Build Mode: {build_mode}")
    print(f"  Version: {version}")

    print()
    print("=" * 60)
    print("✓ SUCCESS! libghostty works with correct bindings!")
    print("=" * 60)
    print()
    print("Root cause identified:")
    print("  • Incorrect C structure definition in cffi")
    print("  • Structure fields were completely wrong")
    print("  • Caused memory corruption and NULL pointer access")
    print()
    print("Solution:")
    print("  • Use correct structure from ghostty.h")
    print("  • Field 1: ghostty_build_mode_e (enum)")
    print("  • Field 2: const char* version")
    print("  • Field 3: uintptr_t version_len")

except Exception as e:
    print(f"✗ Error: {e}")
    import traceback
    traceback.print_exc()
    print()
    print("=" * 60)
    print("✗ Still has issues")
    print("=" * 60)
    sys.exit(1)