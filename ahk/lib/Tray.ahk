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
        for index, group in UiTrayMenu() {
            if index > 1
                menu.Add()
            for item in group {
                action := item[1]
                label := item[2]
                if item[4] != "" && this.app.config.Bool("hotkey_enabled", true)
                    label := item[4]
                added := this.Add(menu, label, item[3], this.Callback(action))
                if action = "settings"
                    menu.Default := added
            }
        }
    }

    Callback(action) {
        switch action {
            case "read_selection", "read_region", "read_along", "copy_region":
                return ObjBindMethod(this.app, "ReaderAction", action)
            case "abort": return ObjBindMethod(this.app, "CancelAll")
            case "settings": return ObjBindMethod(this.app, "OpenSettings")
            case "unload": return ObjBindMethod(this.app, "UnloadAllModels")
            case "toggle_hotkey": return ObjBindMethod(this.app, "ToggleHotkey")
            case "reload_settings": return ObjBindMethod(this.app, "ReloadSettings")
            case "reload_app": return ObjBindMethod(this.app, "ReloadApp")
            case "latest_log": return ObjBindMethod(this.app, "OpenLatestLog")
            case "restart_audio": return ObjBindMethod(this.app, "RestartAudioService")
            case "test": return ObjBindMethod(this.app, "TestModel")
            case "exit": return (*) => ExitApp()
            default: throw Error("Unknown tray action",, action)
        }
    }
}
