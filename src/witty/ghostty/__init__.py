"""
Ghostty Python bindings module.

This module provides Python wrappers for libghostty.
"""

from .bindings import ffi, lib, get_version, get_build_mode
from .app import GhosttyApp
from .surface import GhosttySurface
from .callbacks import GhosttyCallbackManager

__all__ = [
    'ffi',
    'lib',
    'get_version',
    'get_build_mode',
    'GhosttyApp',
    'GhosttySurface',
    'GhosttyCallbackManager',
]

__version__ = "0.1.0"