import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
AHK = r"C:\Program Files\AutoHotkey\v2\AutoHotkey64.exe"
SCRIPT = str(ROOT / "ahk" / "tests" / "settings-preview.ahk")

validate = subprocess.run([AHK, "/ErrorStdOut=UTF-8", "/Validate", SCRIPT])
if validate.returncode:
    raise SystemExit(validate.returncode)

run = subprocess.run([AHK, "/ErrorStdOut=UTF-8", SCRIPT])
raise SystemExit(run.returncode)
