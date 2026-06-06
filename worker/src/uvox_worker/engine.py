"""Session-aware streaming engine with cancellation and conservative commits."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Callable, Protocol

from .constants import DEFAULT_STABILITY_OBSERVATIONS
from .stabilizer import PrefixStabilizer


class Recognizer(Protocol):
    @property
    def config(self): ...
    def reset(self) -> None: ...
    def transcribe_chunk(self, pcm16_bytes: bytes) -> str: ...


EmitFn = Callable[[dict], None]


@dataclass
class EngineState:
    session_id: int | None = None
    pcm_buffer: bytearray | None = None
    last_partial: str = ""


class LiveEngine:
    def __init__(
        self,
        recognizer: Recognizer,
        emit: EmitFn,
        *,
        stability_observations: int = DEFAULT_STABILITY_OBSERVATIONS,
    ) -> None:
        self.recognizer = recognizer
        self.emit = emit
        self.stabilizer = PrefixStabilizer(stability_observations)
        self.state = EngineState(pcm_buffer=bytearray())

    @property
    def chunk_bytes(self) -> int:
        return int(self.recognizer.config.chunk_samples) * 2

    def start(self, session_id: int) -> None:
        self.recognizer.reset()
        self.stabilizer.reset()
        self.state = EngineState(session_id=session_id, pcm_buffer=bytearray())
        self.emit({"type": "session_started", "session_id": session_id})

    def cancel(self, session_id: int) -> None:
        if self.state.session_id != session_id:
            return
        self.state = EngineState(pcm_buffer=bytearray())
        self.stabilizer.reset()
        self.emit({"type": "session_cancelled", "session_id": session_id})

    def push_pcm16(self, session_id: int, pcm: bytes) -> None:
        if session_id != self.state.session_id:
            return
        assert self.state.pcm_buffer is not None
        self.state.pcm_buffer.extend(pcm)
        while len(self.state.pcm_buffer) >= self.chunk_bytes:
            chunk = bytes(self.state.pcm_buffer[: self.chunk_bytes])
            del self.state.pcm_buffer[: self.chunk_bytes]
            partial = self.recognizer.transcribe_chunk(chunk)
            self.state.last_partial = partial
            update = self.stabilizer.observe(partial)
            self.emit({"type": "partial", "session_id": session_id, "text": update.partial})
            if update.commit_delta:
                self.emit({"type": "commit", "session_id": session_id, "text": update.commit_delta})

    def force_finish_for_file_test(self) -> str:
        """Flush the latest hypothesis for CLI validation only, never for live CapsLock release."""
        partial = self.state.last_partial
        update = self.stabilizer.force_commit(partial)
        session_id = self.state.session_id
        if session_id is not None and update.commit_delta:
            self.emit({"type": "commit", "session_id": session_id, "text": update.commit_delta})
        return update.committed
