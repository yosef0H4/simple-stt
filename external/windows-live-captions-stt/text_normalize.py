from __future__ import annotations

import re


_WORD_RE = re.compile(r"[a-z0-9]+")


def normalize_text(text: str) -> str:
    return " ".join(_WORD_RE.findall(text.lower()))


def contains_expected(observed: str, expected: str) -> bool:
    expected_norm = normalize_text(expected)
    observed_norm = normalize_text(observed)
    return bool(expected_norm) and expected_norm in observed_norm
