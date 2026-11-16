class TrayController {
    __New(app) {
        this.app := app
        A_IconTip := UiText("menu.iconTip")
        this.Rebuild()
    }

    Rebuild() {
        A_IconTip := UiText("menu.iconTip")
        menu := A_TrayMenu
        menu.Delete()
        menu.Add(UiText("menu.readSelection"), ObjBindMethod(this.app, "ReaderAction", "read_selection"))
        menu.Add(UiText("menu.readRegion"), ObjBindMethod(this.app, "ReaderAction", "read_region"))
        menu.Add(UiText("menu.copyRegion"), ObjBindMethod(this.app, "ReaderAction", "copy_region"))
        menu.Add("Read along", ObjBindMethod(this.app, "ReaderAction", "read_along"))
        menu.Add(UiText("menu.abort"), ObjBindMethod(this.app, "CancelAll"))
        menu.Add()
        menu.Add(UiText("menu.openSettings"), ObjBindMethod(this.app, "OpenSettings"))
        menu.Default := UiText("menu.openSettings")
        menu.Add(UiText("menu.unloadAll"), ObjBindMethod(this.app, "UnloadAllModels"))
        menu.Add()
        menu.Add(this.app.config.Bool("hotkey_enabled", true) ? UiText("menu.disable") : UiText("menu.enable"), ObjBindMethod(this.app, "ToggleHotkey"))
        menu.Add(UiText("menu.reloadSettings"), ObjBindMethod(this.app, "ReloadSettings"))
        menu.Add(UiText("menu.reloadApp"), ObjBindMethod(this.app, "ReloadApp"))
        menu.Add()
        menu.Add(UiText("menu.latestLog"), ObjBindMethod(this.app, "OpenLatestLog"))
        menu.Add(UiText("menu.restartAudio"), ObjBindMethod(this.app, "RestartAudioService"))
        menu.Add(UiText("menu.test"), ObjBindMethod(this.app, "TestModel"))
        menu.Add()
        menu.Add(UiText("menu.exit"), (*) => ExitApp())
    }
}
