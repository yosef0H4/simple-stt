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
        menu.Add(UiText("menu.openSettings"), ObjBindMethod(this.app, "OpenSettings"))
        menu.Default := UiText("menu.openSettings")
        menu.Add(this.app.config.Bool("hotkey_enabled", true) ? UiText("menu.disable") : UiText("menu.enable"), ObjBindMethod(this.app, "ToggleHotkey"))
        menu.Add(UiText("menu.reloadSettings"), ObjBindMethod(this.app, "ReloadSettings"))
        menu.Add(UiText("menu.reloadApp"), ObjBindMethod(this.app, "ReloadApp"))
        menu.Add()
        menu.Add(UiText("menu.latestLog"), ObjBindMethod(this.app, "OpenLatestLog"))
        menu.Add(UiText("menu.restartAudio"), ObjBindMethod(this.app, "RestartAudioService"))
        menu.Add(UiText("menu.unload"), ObjBindMethod(this.app, "UnloadSpeechModel"))
        menu.Add(UiText("menu.test"), ObjBindMethod(this.app, "TestModel"))
        menu.Add()
        menu.Add(UiText("menu.exit"), (*) => ExitApp())
    }
}
