#!/usr/bin/env python3
"""Build resources/bin/linux-fast-paste when Linux development headers exist.

This script is best-effort: if optional XTest/GIO/AT-SPI/uinput headers are not
installed, it prints a clear warning and exits 0 so Rust tests are not blocked.
"""
from __future__ import annotations

import hashlib
import os
import shutil
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SRC = ROOT / "resources" / "linux-fast-paste.c"
OUT_DIR = ROOT / "resources" / "bin"
OUT = OUT_DIR / "linux-fast-paste"
HASH = OUT_DIR / ".linux-fast-paste.hash"


def sh(cmd: list[str], **kwargs) -> subprocess.CompletedProcess[str]:
    return subprocess.run(cmd, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE, **kwargs)


def pkg_exists(name: str) -> bool:
    return shutil.which("pkg-config") is not None and subprocess.run(
        ["pkg-config", "--exists", name], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL
    ).returncode == 0


def pkg_flags(name: str) -> list[str]:
    out: list[str] = []
    for flag in ("--cflags", "--libs"):
        proc = sh(["pkg-config", flag, name])
        if proc.returncode == 0:
            out.extend(proc.stdout.strip().split())
    return out


def can_include(header: str) -> bool:
    cc = shutil.which("gcc") or shutil.which("cc")
    if not cc:
        return False
    proc = subprocess.run(
        [cc, "-E", "-x", "c", "-"],
        input=f"#include <{header}>\n",
        text=True,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    return proc.returncode == 0


def source_hash(flags: list[str]) -> str:
    h = hashlib.sha256()
    h.update(SRC.read_bytes())
    h.update("\0".join(flags).encode())
    return h.hexdigest()


def main() -> int:
    if os.name != "posix" or not sys_platform_linux():
        return 0
    if not SRC.exists():
        print(f"[linux-fast-paste] missing {SRC}")
        return 0
    cc = shutil.which("gcc") or shutil.which("cc")
    if not cc:
        print("[linux-fast-paste] no C compiler found; skipping")
        return 0
    if not (pkg_exists("x11") and pkg_exists("xtst")):
        print("[linux-fast-paste] libx11/libxtst development packages missing; skipping native helper")
        return 0

    flags = ["-O2", str(SRC), "-o", str(OUT), *pkg_flags("x11"), *pkg_flags("xtst")]
    if can_include("linux/uinput.h"):
        flags.append("-DHAVE_UINPUT")
    if pkg_exists("gio-2.0"):
        flags.extend(["-DHAVE_GIO", *pkg_flags("gio-2.0")])
    if pkg_exists("atspi-2"):
        flags.extend(["-DHAVE_ATSPI", *pkg_flags("atspi-2")])

    OUT_DIR.mkdir(parents=True, exist_ok=True)
    digest = source_hash(flags)
    if OUT.exists() and HASH.exists() and HASH.read_text().strip() == digest:
        print("[linux-fast-paste] already up to date")
        return 0
    print("[linux-fast-paste] compiling", cc, " ".join(flags))
    proc = subprocess.run([cc, *flags])
    if proc.returncode != 0:
        print("[linux-fast-paste] compile failed; automatic paste will use wl-copy/wtype/ydotool/xdotool fallbacks")
        return 0
    OUT.chmod(0o755)
    HASH.write_text(digest + "\n")
    print(f"[linux-fast-paste] built {OUT}")
    return 0


def sys_platform_linux() -> bool:
    import platform
    return platform.system().lower() == "linux"


if __name__ == "__main__":
    raise SystemExit(main())
