#!/usr/bin/env python3
"""Refresh the extracted build tree after reproducible archive timestamps.

Cargo's local source checks use mtimes. Each release snapshot has the same
commit timestamp until committed, so a reused target cache can otherwise miss
edits between local preview packages. Only the disposable source copy is touched.
"""
import argparse
from pathlib import Path


def refresh(directory):
    for path in directory.rglob('*'):
        if path.is_file() and not path.is_symlink():
            path.touch()


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('directory', type=Path)
    args = parser.parse_args()
    refresh(args.directory.resolve(strict=True))
