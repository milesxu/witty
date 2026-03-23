#!/usr/bin/env python3
"""Test loading the newly compiled debug version of libghostty."""

import sys
from cffi import FFI

print("=" * 60)
print("Testing Debug Build of libghostty")
print("=" * 60)
print()

# Initialize FFI
ffi = FFI()
ffi.cdef("""
    typedef struct {
        const char* version;
        uint32_t version_major;
        uint32_t version_minor;
        uint32_t version_patch;
    } ghostty_info_s;

    ghostty_info_s ghostty_info(void);
""")

# Try loading the debug build
lib_path = "/home/xuming/src/ghostty/zig-out/lib/libghostty.so"

print(f"Loading: {lib_path}")
print()

try:
    lib = ffi.dlopen(lib_path)
    print("✓ Library loaded successfully!")
    print()

    print("Testing ghostty_info()...")
    info = lib.ghostty_info()

    version = ffi.string(info.version).decode('utf-8')
    print(f"  Version: {version}")
    print(f"  Major: {info.version_major}")
    print(f"  Minor: {info.version_minor}")
    print(f"  Patch: {info.version_patch}")

    print()
    print("=" * 60)
    print("✓ SUCCESS! Debug build works!")
    print("=" * 60)

except Exception as e:
    print(f"✗ Error: {e}")
    import traceback
    traceback.print_exc()
    print()
    print("=" * 60)
    print("✗ Debug build still has issues")
    print("=" * 60)
    sys.exit(1)