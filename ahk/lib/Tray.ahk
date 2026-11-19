class TrayController {
    static IconDirectory := A_ScriptDir . "\icons"

    __New(app) {
        this.app := app
        icon := TrayController.IconDirectory . "\simple-stt.ico"
        if FileExist(icon)
            TraySetIcon(icon)
        this.Rebuild()
    }

    Add(menu, id, icon, callback) {
        label := UiText(id)
        menu.Add(label, callback)
        path := TrayController.IconDirectory . "\" . icon . ".ico"
        if FileExist(path)
            menu.SetIcon(label, path)
        return label
    }

    Rebuild() {
        A_IconTip := UiText("menu.iconTip")
        menu := A_TrayMenu
        menu.Delete()
        this.Add(menu, "menu.readSelection", "speaker", ObjBindMethod(this.app, "ReaderAction", "read_selection"))
        this.Add(menu, "menu.readRegion", "camera", ObjBindMethod(this.app, "ReaderAction", "read_region"))
        this.Add(menu, "reading.read_along_start", "book", ObjBindMethod(this.app, "ReaderAction", "read_along"))
        this.Add(menu, "menu.copyRegion", "clipboard", ObjBindMethod(this.app, "ReaderAction", "copy_region"))
        this.Add(menu, "menu.abort", "stop", ObjBindMethod(this.app, "CancelAll"))
        menu.Add()
        menu.Default := this.Add(menu, "menu.openSettings", "settings", ObjBindMethod(this.app, "OpenSettings"))
        this.Add(menu, "menu.unloadAll", "unload", ObjBindMethod(this.app, "UnloadAllModels"))
        menu.Add()
        this.Add(menu, this.app.config.Bool("hotkey_enabled", true) ? "menu.disable" : "menu.enable", "keyboard", ObjBindMethod(this.app, "ToggleHotkey"))
        this.Add(menu, "menu.reloadSettings", "reload", ObjBindMethod(this.app, "ReloadSettings"))
        this.Add(menu, "menu.reloadApp", "reload", ObjBindMethod(this.app, "ReloadApp"))
        menu.Add()
        this.Add(menu, "menu.latestLog", "log", ObjBindMethod(this.app, "OpenLatestLog"))
        this.Add(menu, "menu.restartAudio", "speaker", ObjBindMethod(this.app, "RestartAudioService"))
        this.Add(menu, "menu.test", "test", ObjBindMethod(this.app, "TestModel"))
        menu.Add()
        this.Add(menu, "menu.exit", "exit", (*) => ExitApp())
    }
}
