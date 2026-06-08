#Requires AutoHotkey v2.0
#SingleInstance Force

#Include ..\lib\Utils.ahk
#Include ..\lib\Logging.ahk
#Include ..\lib\TabProtocol.ahk
#Include ..\lib\Config.ahk
#Include ..\lib\Hotkeys.ahk
#Include ..\lib\SettingsGui.ahk

class FakeIpc {
    CallService(arguments, callback := "", kind := "command") {
        return 0
    }
}

class FakeApp {
    __New() {
        ctlExe := UvoxResolveExe("uvoxctl")
        this.config := ConfigStore(ctlExe)
        this.logger := ShellLog(A_Temp . "\uvox-settings-smoke.log")
        this.ipc := FakeIpc()
    }

    OpenLatestLog(*) {
    }

    ApplySavedConfig(*) {
    }

    TestModel(*) {
    }
}

Fail(message, exitCode := 1) {
    UvoxConsoleError("FAIL: " . message)
    ExitApp(exitCode)
}

app := FakeApp()
settings := SettingsGui(app)

try settings.Open()
catch Error as err
    Fail("SettingsGui.Open() failed: " . err.Message)

if !IsObject(settings.gui)
    Fail("settings GUI object was not created")

for key in ["hotkey_enabled", "record_hotkey", "capslock_behavior", "audio_device_contains", "selected_model_filename", "status"] {
    if !settings.controls.Has(key)
        Fail("missing settings control: " . key)
}

settings.Hide()
try settings.gui.Destroy()

UvoxConsoleLine("PASS: settings GUI open smoke")
ExitApp(0)
