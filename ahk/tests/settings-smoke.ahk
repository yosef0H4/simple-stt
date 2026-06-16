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
        this.testMode := true
        ctlExe := SimpleSttResolveExe("simple-stt-ctl")
        this.config := ConfigStore(ctlExe)
        this.logger := ShellLog(A_Temp . "\simple-stt-settings-smoke.log")
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
    SimpleSttConsoleError("FAIL: " . message)
    ExitApp(exitCode)
}

tempDir := A_Temp . "\simple-stt-settings-smoke-" . A_TickCount
DirCreate(tempDir)
EnvSet("SIMPLE_STT_CONFIG", tempDir . "\config.json")

app := FakeApp()
settings := SettingsGui(app)

try settings.Open()
catch Error as err
    Fail("SettingsGui.Open() failed: " . err.Message)

if !IsObject(settings.gui)
    Fail("settings GUI object was not created")

for key in ["hotkey_enabled", "record_hotkey", "cancel_hotkey", "toggle_delivery_hotkey", "capslock_behavior", "audio_device_contains", "selected_model_filename", "model_list", "inference_device", "text_delivery_mode", "remove_punctuation", "lowercase_output", "status"] {
    if !settings.controls.Has(key)
        Fail("missing settings control: " . key)
}

settings.controls["typing_chunk_chars"].Value := "4"
settings.controls["cancel_hotkey"].Text := "CapsLock+A"
settings.controls["toggle_delivery_hotkey"].Text := "CapsLock+D"
settings.controls["audio_device_contains"].Choose("Default microphone")
settings.controls["inference_device"].Choose("cpu")
settings.controls["text_delivery_mode"].Choose("paste_ctrl_shift_v")
settings.controls["remove_punctuation"].Value := 1
settings.controls["lowercase_output"].Value := 1
try settings.Save()
catch Error as err
    Fail("SettingsGui.Save() failed: " . err.Message)
app.config.LoadSync()
if app.config.Get("typing_chunk_chars") != "4"
    Fail("settings save did not persist typing_chunk_chars")
if app.config.Get("cancel_hotkey") != "CapsLock+A"
    Fail("settings save did not persist cancel_hotkey")
if app.config.Get("toggle_delivery_hotkey") != "CapsLock+D"
    Fail("settings save did not persist toggle_delivery_hotkey")
if app.config.Get("inference_device") != "cpu"
    Fail("settings save did not persist inference_device")
if app.config.Get("text_delivery_mode") != "paste_ctrl_shift_v"
    Fail("settings save did not persist text_delivery_mode")
if !app.config.Bool("remove_punctuation")
    Fail("settings save did not persist remove_punctuation")
if !app.config.Bool("lowercase_output")
    Fail("settings save did not persist lowercase_output")

settings.Hide()
try settings.gui.Destroy()

SimpleSttConsoleLine("PASS: settings GUI open/save smoke")
ExitApp(0)
