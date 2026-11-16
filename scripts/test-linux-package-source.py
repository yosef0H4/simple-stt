#!/usr/bin/env python3
"""Prove repeated preview packages rebuild edits with equal archive mtimes."""
import importlib.util
import os
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('prepare', ROOT / 'scripts/prepare-linux-package-source.py')
prepare = importlib.util.module_from_spec(spec)
spec.loader.exec_module(prepare)

with tempfile.TemporaryDirectory(prefix='simple-stt-package-source-') as folder:
    root = Path(folder) / 'source'
    root.mkdir()
    target = Path(folder) / 'target'
    (root / 'src').mkdir()
    (root / 'Cargo.toml').write_text('[package]\nname="package-source-probe"\nversion="0.1.0"\nedition="2021"\n')
    source = root / 'src/main.rs'
    source.write_text('fn main() { println!("first"); }\n')
    archive_time = 946684800
    os.utime(source, (archive_time, archive_time))
    env = dict(os.environ, CARGO_TARGET_DIR=str(target))

    def build():
        result = subprocess.run(['cargo', 'build', '--release', '--offline', '--quiet'],
                                cwd=root, env=env, capture_output=True, text=True, timeout=60)
        assert result.returncode == 0, result.stderr
        return subprocess.check_output([str(target / 'release/package-source-probe')], text=True).strip()

    assert build() == 'first'
    source.write_text('fn main() { println!("second"); }\n')
    os.utime(source, (archive_time, archive_time))
    assert build() == 'first', 'regression setup did not reproduce Cargo timestamp reuse'
    prepare.refresh(root)
    assert build() == 'second', 'preview package reused stale source'
    print('PASS equal archive timestamps reproduce stale Cargo output; source refresh rebuilds edits')
