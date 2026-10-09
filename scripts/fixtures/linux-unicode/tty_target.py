#!/usr/bin/env python3
"""Raw TTY sink used only by the isolated Kitty integration test."""
import os
import sys
import termios
import tty

ready_path, output_path, byte_count = sys.argv[1], sys.argv[2], int(sys.argv[3])
fd = sys.stdin.fileno()
original = termios.tcgetattr(fd)
try:
    tty.setraw(fd)
    with open(ready_path, "w", encoding="ascii") as ready:
        ready.write("ready\n")
    chunks = []
    remaining = byte_count
    while remaining:
        chunk = os.read(fd, remaining)
        if not chunk:
            break
        chunks.append(chunk)
        remaining -= len(chunk)
        with open(output_path, "wb") as output:
            output.write(b"".join(chunks))
finally:
    termios.tcsetattr(fd, termios.TCSANOW, original)
