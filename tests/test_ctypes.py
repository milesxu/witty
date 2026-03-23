#!/usr/bin/env python3
"""Test loading libghostty with ctypes instead of cffi."""

import ctypes
from ctypes import Structure, c_char_p, c_uint32, POINTER

print("Step 1: Defining structures...")

class GhosttyInfo(Structure):
    _fields_ = [
        ("version", c_char_p),
        ("version_major", c_uint32),
        ("version_minor", c_uint32),
        ("version_patch", c_uint32),
    ]

print("Step 2: Loading library...")
try:
    lib = ctypes.CDLL("/usr/local/lib/libghostty.so")
    print("✓ Library loaded with ctypes!")
except Exception as e:
    print(f"✗ Failed to load: {e}")
    exit(1)

print("\nStep 3: Setting up function signature...")
try:
    lib.ghostty_info.restype = GhosttyInfo
    lib.ghostty_info.argtypes = []
    print("✓ Function signature configured")
except Exception as e:
    print(f"✗ Error setting signature: {e}")
    exit(1)

print("\nStep 4: Calling ghostty_info()...")
try:
    info = lib.ghostty_info()
    print("✓ Function called successfully!")

    version = info.version.decode('utf-8')
    print(f"\nGhostty Version: {version}")
    print(f"  Major: {info.version_major}")
    print(f"  Minor: {info.version_minor}")
    print(f"  Patch: {info.version_patch}")

    print("\n✓ All tests passed with ctypes!")
except Exception as e:
    print(f"✗ Error calling function: {e}")
    import traceback
    traceback.print_exc()
    exit(1)