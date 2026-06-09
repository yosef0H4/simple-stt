class TrayController {
    __New(app) {
        this.app := app
        try TraySetIcon("shell32.dll", 44)
        A_IconTip := "Uvox local dictation"
        this.Rebuild()
    }

    Rebuild() {
        menu := A_TrayMenu
        menu.Delete()
        menu.Add("Open Settings", ObjBindMethod(this.app, "OpenSettings"))
        menu.Default := "Open Settings"
        menu.Add(this.app.config.Bool("hotkey_enabled", true) ? "Disable Hotkey" : "Enable Hotkey", ObjBindMethod(this.app, "ToggleHotkey"))
        menu.Add("Reload Settings", ObjBindMethod(this.app, "ReloadSettings"))
        menu.Add("Reload App", ObjBindMethod(this.app, "ReloadApp"))
        menu.Add()
        menu.Add("Open Latest Log", ObjBindMethod(this.app, "OpenLatestLog"))
        menu.Add("Restart Audio Service", ObjBindMethod(this.app, "RestartAudioService"))
        menu.Add("Unload Speech Model", ObjBindMethod(this.app, "UnloadSpeechModel"))
        menu.Add("Test Model", ObjBindMethod(this.app, "TestModel"))
        menu.Add()
        menu.Add("Exit", (*) => ExitApp())
    }
}
