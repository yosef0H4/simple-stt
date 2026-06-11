import subprocess
import sys

AHK = r"C:\Program Files\AutoHotkey\v2\AutoHotkey64.exe"
SCRIPT = r"Z:\files\projects\rust\simple-stt\ahk\tests\settings-preview.ahk"

validate = subprocess.run([AHK, "/ErrorStdOut=UTF-8", "/Validate", SCRIPT])
if validate.returncode:
    raise SystemExit(validate.returncode)

run = subprocess.run([AHK, "/ErrorStdOut=UTF-8", SCRIPT])
raise SystemExit(run.returncode)
