from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
path = ROOT / "ahk/lib/SettingsGui.ahk"
text = path.read_text(encoding="utf-8")

def replace_one(old: str, new: str) -> None:
    global text
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"expected one match, found {count}: {old[:80]!r}")
    text = text.replace(old, new, 1)

replace_one(
'''        if IsObject(this.gui) {
            this.gui.Show()
            return
        }''',
'''        if IsObject(this.gui) {
            this.gui.Show()
            this.ListInputs()
            this.ListModels()
            return
        }''')
replace_one(
'''        refreshMic := window.AddButton("x+8 yp-1 w120", "List microphones")''',
'''        refreshMic := window.AddButton("x+8 yp-1 w120", "Refresh microphones")''')
replace_one(
'''        listModels := window.AddButton("x+8 yp-1 w120", "List models")
        listModels.OnEvent("Click", ObjBindMethod(this, "ListModels"))''',
'''        listModels := window.AddButton("x+8 yp-1 w120", "Refresh models")
        listModels.OnEvent("Click", ObjBindMethod(this, "RefreshModels"))''')
replace_one(
'''        this.controls["model_dir"] := window.AddEdit("x+8 yp-3 w500")''',
'''        this.controls["model_dir"] := window.AddEdit("x+8 yp-3 w360")
        browseModels := window.AddButton("x+8 yp-1 w80", "Browse")
        browseModels.OnEvent("Click", ObjBindMethod(this, "BrowseModelDir"))
        openModels := window.AddButton("x+8 yp w90", "Open folder")
        openModels.OnEvent("Click", ObjBindMethod(this, "OpenModelDir"))''')
