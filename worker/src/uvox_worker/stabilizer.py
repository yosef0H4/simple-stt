"""Convert revisable ASR hypotheses into conservative incremental commits."""

from __future__ import annotations

from collections import deque
from dataclasses import dataclass


def longest_common_prefix(values: list[str]) -> str:
    if not values:
        return ""
    prefix = values[0]
    for value in values[1:]:
        limit = min(len(prefix), len(value))
        index = 0
        while index < limit and prefix[index] == value[index]:
            index += 1
        prefix = prefix[:index]
        if not prefix:
            break
    return prefix


def complete_text_boundary(text: str) -> str:
    """Keep only complete words, preserving trailing whitespace after committed words."""
    if not text:
        return ""
    if text[-1].isspace():
        return text
    last_space = max(text.rfind(" "), text.rfind("\n"), text.rfind("\t"))
    return text[: last_space + 1] if last_space >= 0 else ""


@dataclass(frozen=True)
class StabilizedUpdate:
    partial: str
    commit_delta: str
    committed: str


class PrefixStabilizer:
    """Commit a word boundary only after it appears in multiple consecutive hypotheses."""

    def __init__(self, required_observations: int = 3) -> None:
        if required_observations < 1:
            raise ValueError("required_observations must be positive")
        self.required_observations = required_observations
        self._history: deque[str] = deque(maxlen=required_observations)
        self._committed = ""

    @property
    def committed(self) -> str:
        return self._committed

    def reset(self) -> None:
        self._history.clear()
        self._committed = ""

    def observe(self, hypothesis: str) -> StabilizedUpdate:
        self._history.append(hypothesis)
        delta = ""
        if len(self._history) == self.required_observations:
            safe = complete_text_boundary(longest_common_prefix(list(self._history)))
            if safe.startswith(self._committed) and len(safe) > len(self._committed):
                delta = safe[len(self._committed) :]
                self._committed = safe
        return StabilizedUpdate(partial=hypothesis, commit_delta=delta, committed=self._committed)

    def force_commit(self, hypothesis: str) -> StabilizedUpdate:
        """Commit final text for file tests only; live CapsLock release deliberately does not call this."""
        delta = ""
        if hypothesis.startswith(self._committed):
            delta = hypothesis[len(self._committed) :]
            self._committed = hypothesis
        return StabilizedUpdate(partial=hypothesis, commit_delta=delta, committed=self._committed)
