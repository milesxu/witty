#!/usr/bin/env python3
"""Test loading libghostty-vt.so (terminal emulation only, no rendering)."""

import sys
from cffi import FFI

print("=" * 60)
print("Testing libghostty-vt.so")
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
""")

# Try loading libghostty-vt
lib_path = "/home/xuming/src/ghostty/zig-out/lib/libghostty-vt.so"

print(f"Loading: {lib_path}")
print()

try:
    lib = ffi.dlopen(lib_path)
    print("✓ libghostty-vt.so loaded successfully!")
    print()
    print("=" * 60)
    print("✓ SUCCESS! libghostty-vt works!")
    print("=" * 60)
    print()
    print("This means we can:")
    print("  • Use libghostty-vt for terminal emulation")
    print("  • Implement our own OpenGL rendering")
    print("  • Continue with the integration")

except Exception as e:
    print(f"✗ Error: {e}")
    import traceback
    traceback.print_exc()
    print()
    print("=" * 60)
    print("✗ libghostty-vt also has issues")
    print("=" * 60)
    sys.exit(1)