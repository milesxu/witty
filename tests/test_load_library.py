#!/usr/bin/env python3
"""Minimal test - just try to load the library."""

from cffi import FFI

print("Step 1: Creating FFI instance...")
ffi = FFI()

print("Step 2: Defining minimal C API...")
ffi.cdef("""
    typedef struct {
        const char* version;
        uint32_t version_major;
        uint32_t version_minor;
        uint32_t version_patch;
    } ghostty_info_s;

    ghostty_info_s ghostty_info(void);
""")

print("Step 3: Attempting to load libghostty.so...")
try:
    lib = ffi.dlopen("/usr/local/lib/libghostty.so")
    print("✓ Library loaded successfully!")
except Exception as e:
    print(f"✗ Failed to load library: {e}")
    exit(1)

print("\nStep 4: Testing ghostty_info() call...")
try:
    info = lib.ghostty_info()
    print("✓ ghostty_info() called successfully!")

    version = ffi.string(info.version).decode('utf-8')
    print(f"\nGhostty Version: {version}")
    print(f"  Major: {info.version_major}")
    print(f"  Minor: {info.version_minor}")
    print(f"  Patch: {info.version_patch}")

    print("\n✓ All tests passed!")
except Exception as e:
    print(f"✗ Error calling ghostty_info(): {e}")
    import traceback
    traceback.print_exc()
    exit(1)