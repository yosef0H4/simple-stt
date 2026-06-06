"""Typed errors with actionable messages for users and coding agents."""

from __future__ import annotations


class UvoxError(RuntimeError):
    """Base exception for expected Uvox failures."""


class CudaRequiredError(UvoxError):
    """Raised when the CUDA-only worker cannot use an NVIDIA CUDA device."""


class AudioFormatError(UvoxError):
    """Raised when an audio file is not suitable for the recognizer."""


class ProtocolError(UvoxError):
    """Raised for malformed IPC frames or messages."""
