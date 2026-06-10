class SettingsGui {
    __New(app) {
        this.app := app
        this.gui := ""
        this.controls := Map()
        this.recorder := HotkeyRecorder(app.logger)
    }

    Open(*) {
        if IsObject(this.gui) {
            this.gui.Show()
            this.ListInputs()
            this.ListModels()
            return
        }
        window := Gui("+Resize", "SimpleStt Settings")
        this.gui := window
        window.SetFont("s9", "Segoe UI")
        window.OnEvent("Close", ObjBindMethod(this, "Hide"))
        window.AddText("xm ym w720", "Immediate: hotkey, typing pace, trailing space, Caps Lock, startup. Restart audio service: microphone and gain. Speech worker is recycled automatically after model or idle-timeout changes.")

        this.controls["hotkey_enabled"] := window.AddCheckbox("xm y+14", "Hotkey enabled")
        window.AddText("xm y+12 w150", "Hold-to-record hotkey")
        this.controls["record_hotkey"] := window.AddEdit("x+8 yp-3 w250")
        captureButton := window.AddButton("x+8 yp-1 w115", "Record chord")
        captureButton.OnEvent("Click", ObjBindMethod(this, "CaptureHotkey"))
        window.AddText("xm y+12 w150", "Caps Lock behavior")
        this.controls["capslock_behavior"] := window.AddDropDownList("x+8 yp-3 w250", ["preserve_tap", "always_off"])

        window.AddText("xm y+18 w150", "Microphone contains")
        this.controls["audio_device_contains"] := window.AddEdit("x+8 yp-3 w360")
        refreshMic := window.AddButton("x+8 yp-1 w120", "List microphones")
        refreshMic.OnEvent("Click", ObjBindMethod(this, "ListInputs"))
        this.controls["input_list"] := window.AddDropDownList("xm y+8 w640", ["Use default microphone"])
        this.controls["input_list"].OnEvent("Change", ObjBindMethod(this, "ChooseInput"))
        window.AddText("xm y+12 w150", "Audio gain")
        this.controls["audio_gain"] := window.AddEdit("x+8 yp-3 w100")

        window.AddText("xm y+18 w150", "Selected model")
        this.controls["selected_model_filename"] := window.AddEdit("x+8 yp-3 w360")
        listModels := window.AddButton("x+8 yp-1 w120", "Refresh models")
        listModels.OnEvent("Click", ObjBindMethod(this, "RefreshModels"))
        this.controls["model_list"] := window.AddDropDownList("xm y+8 w640", ["Load approved model list"])
        this.controls["model_list"].OnEvent("Change", ObjBindMethod(this, "ChooseModel"))
        downloadButton := window.AddButton("xm y+8 w150", "Download model")
        downloadButton.OnEvent("Click", ObjBindMethod(this, "DownloadModel"))
        testButton := window.AddButton("x+8 yp w150", "Test model")
        testButton.OnEvent("Click", ObjBindMethod(this, "TestModel"))

        window.AddText("xm y+18 w150", "Text delivery")
        this.controls["text_delivery_mode"] := window.AddDropDownList("x+8 yp-3 w220", ["type", "paste_ctrl_v", "paste_ctrl_shift_v"])
        window.AddText("xm y+12 w150", "Typing chunk chars")
        this.controls["typing_chunk_chars"] := window.AddEdit("x+8 yp-3 w100")
        window.AddText("x+20 yp+3 w130", "Typing interval ms")
        this.controls["typing_interval_ms"] := window.AddEdit("x+8 yp-3 w100")
        this.controls["trailing_space"] := window.AddCheckbox("xm y+10", "Append trailing space")
        this.controls["remove_punctuation"] := window.AddCheckbox("xm y+8", "Remove punctuation marks")
        this.controls["lowercase_output"] := window.AddCheckbox("x+20 yp", "Convert output to lowercase")

        window.AddText("xm y+18 w190", "Idle speech-worker timeout sec")
        this.controls["idle_worker_timeout_secs"] := window.AddEdit("x+8 yp-3 w100")
        window.AddText("x+20 yp+3 w160", "Shutdown grace ms")
        this.controls["worker_shutdown_grace_ms"] := window.AddEdit("x+8 yp-3 w100")
        this.controls["start_with_windows"] := window.AddCheckbox("xm y+10", "Start SimpleStt shell with Windows")
        window.AddText("xm y+12 w150", "Logging level")
        this.controls["log_level"] := window.AddDropDownList("x+8 yp-3 w160", ["minimal", "normal", "debug", "extreme"])
        this.controls["diagnostic_overlay"] := window.AddCheckbox("xm y+10", "Diagnostic overlay notices")
        this.controls["log_transcripts"] := window.AddCheckbox("x+20 yp", "Diagnostic transcript logging")

        window.AddText("xm y+18 w190", "Advanced runtime directory")
        this.controls["parakeet_runtime_dir"] := window.AddEdit("x+8 yp-3 w500")
        window.AddText("xm y+12 w190", "Advanced model directory")
        this.controls["model_dir"] := window.AddEdit("x+8 yp-3 w360")
        browseModels := window.AddButton("x+8 yp-1 w80", "Browse")
        browseModels.OnEvent("Click", ObjBindMethod(this, "BrowseModelDir"))
        openModels := window.AddButton("x+8 yp w90", "Open folder")
        openModels.OnEvent("Click", ObjBindMethod(this, "OpenModelDir"))

        save := window.AddButton("xm y+20 w120 Default", "Save")
        save.OnEvent("Click", ObjBindMethod(this, "Save"))
        reload := window.AddButton("x+8 yp w120", "Reload")
        reload.OnEvent("Click", ObjBindMethod(this, "Reload"))
        openLog := window.AddButton("x+8 yp w120", "Open log")
        openLog.OnEvent("Click", ObjBindMethod(this.app, "OpenLatestLog"))
        this.controls["status"] := window.AddText("xm y+14 w720", "Ready")
        this.LoadControls()
        window.Show("w760 h860")
        this.ListInputs()
        this.ListModels()
    }

    Hide(*) {
        if IsObject(this.gui)
            this.gui.Hide()
    }

    LoadControls() {
        config := this.app.config
        this.controls["hotkey_enabled"].Value := config.Bool("hotkey_enabled")
        this.controls["record_hotkey"].Value := config.Get("record_hotkey")
        this.ChooseText(this.controls["capslock_behavior"], config.Get("capslock_behavior", "preserve_tap"))
        this.controls["audio_device_contains"].Value := config.Get("audio_device_contains")
        this.controls["audio_gain"].Value := config.Get("audio_gain", "1")
        this.controls["selected_model_filename"].Value := config.Get("selected_model_filename")
        this.ChooseText(this.controls["text_delivery_mode"], config.Get("text_delivery_mode", "paste_ctrl_v"))
        this.controls["typing_chunk_chars"].Value := config.Get("typing_chunk_chars", "3")
        this.controls["typing_interval_ms"].Value := config.Get("typing_interval_ms", "20")
        this.controls["trailing_space"].Value := config.Bool("trailing_space", true)
        this.controls["remove_punctuation"].Value := config.Bool("remove_punctuation")
        this.controls["lowercase_output"].Value := config.Bool("lowercase_output")
        this.controls["idle_worker_timeout_secs"].Value := config.Get("idle_worker_timeout_secs", "180")
        this.controls["worker_shutdown_grace_ms"].Value := config.Get("worker_shutdown_grace_ms", "2000")
        this.controls["start_with_windows"].Value := config.Bool("start_with_windows")
        this.ChooseText(this.controls["log_level"], config.Get("log_level", "normal"))
        this.controls["diagnostic_overlay"].Value := config.Bool("diagnostic_overlay")
        this.controls["log_transcripts"].Value := config.Bool("log_transcripts")
        this.controls["parakeet_runtime_dir"].Value := config.Get("parakeet_runtime_dir")
        this.controls["model_dir"].Value := config.Get("model_dir")
    }

    ChooseText(control, text) {
        try control.Choose(text)
        catch
            control.Choose(1)
        if control.Text != text
            control.Choose(1)
    }

    Save(*) {
        config := this.app.config
        for key in ["record_hotkey", "audio_device_contains", "audio_gain", "selected_model_filename", "typing_chunk_chars", "typing_interval_ms", "idle_worker_timeout_secs", "worker_shutdown_grace_ms", "parakeet_runtime_dir", "model_dir"]
            config.Set(key, this.controls[key].Value)
        for key in ["hotkey_enabled", "trailing_space", "remove_punctuation", "lowercase_output", "start_with_windows", "diagnostic_overlay", "log_transcripts"]
            config.Set(key, SimpleSttBoolText(this.controls[key].Value))
        config.Set("capslock_behavior", this.controls["capslock_behavior"].Text)
        config.Set("text_delivery_mode", this.controls["text_delivery_mode"].Text)
        config.Set("log_level", this.controls["log_level"].Text)
        try {
            HotkeySpec.Parse(config.Get("record_hotkey"))
            config.SaveSync()
            this.app.ApplySavedConfig()
            this.SetStatus("Settings saved")
        } catch Error as err {
            this.SetStatus("Save failed: " . err.Message)
            if this.app.HasProp("testMode") && this.app.testMode
                throw err
            MsgBox(err.Message, "SimpleStt settings error", "Iconx")
        }
    }

    Reload(*) {
        try {
            this.app.config.LoadSync()
            this.LoadControls()
            this.SetStatus("Settings reloaded")
        } catch Error as err {
            this.SetStatus("Reload failed: " . err.Message)
        }
    }

    CaptureHotkey(*) {
        this.SetStatus("Hold the desired modifiers, then press the final key. AltGr is recorded as RAlt; change the text to AltGr when desired.")
        this.recorder.Start(ObjBindMethod(this, "HotkeyCaptured"))
    }
    HotkeyCaptured(label) {
        this.controls["record_hotkey"].Value := label
        this.SetStatus("Recorded " . label . ". Press Save to apply it.")
    }

    ListInputs(*) {
        this.SetStatus("Loading microphones…")
        this.app.ipc.CallService("list-inputs", ObjBindMethod(this, "InputsLoaded"))
    }
    InputsLoaded(response) {
        if !response["ok"] {
            this.SetStatus("Microphone list failed: " . response["message"])
            return
        }
        values := ["Use default microphone"]
        for key, value in response["values"]
            if InStr(key, "input.") = 1
                values.Push(value)
        this.controls["input_list"].Delete()
        this.controls["input_list"].Add(values)
        this.controls["input_list"].Choose(1)
        this.SetStatus("Microphone list refreshed")
    }
    ChooseInput(*) {
        value := this.controls["input_list"].Text
        this.controls["audio_device_contains"].Value := value = "Use default microphone" ? "" : value
    }

    ListModels(*) {
        this.SetStatus("Loading cached models…")
        this.app.ipc.CallService("list-models", ObjBindMethod(this, "ModelsLoaded"))
    }
    RefreshModels(*) {
        this.SetStatus("Refreshing online model catalog…")
        this.app.ipc.CallService("refresh-models", ObjBindMethod(this, "ModelsRefreshed"))
    }
    ModelsRefreshed(response) {
        if !response["ok"] {
            this.SetStatus("Model refresh failed; showing cached list: " . response["message"])
            this.ListModels()
            return
        }
        this.SetStatus("Online model catalog refreshed")
        this.ListModels()
    }
    ModelsLoaded(response) {
        if !response["ok"] {
            this.SetStatus("Model list failed: " . response["message"])
            return
        }
        values := Array()
        for key, value in response["values"]
            if InStr(key, "model.") = 1
                values.Push(value)
        this.controls["model_list"].Delete()
        this.controls["model_list"].Add(values)
        if values.Length
            this.controls["model_list"].Choose(1)
        this.SetStatus("Approved model list refreshed")
    }
    ChooseModel(*) {
        value := this.controls["model_list"].Text
        if value != ""
            this.controls["selected_model_filename"].Value := StrSplit(value, "|")[1]
    }
    DownloadModel(*) {
        filename := this.controls["selected_model_filename"].Value
        this.SetStatus("Model download queued: " . filename)
        this.app.ipc.CallService("download-model --filename " . SimpleSttQuote(filename))
    }
    TestModel(*) {
        this.SetStatus("Model test queued")
        this.app.TestModel()
    }
    BrowseModelDir(*) {
        current := this.controls["model_dir"].Value
        selected := DirSelect(current, 1, "Choose the SimpleStt model folder")
        if selected != ""
            this.controls["model_dir"].Value := selected
    }
    OpenModelDir(*) {
        path := this.controls["model_dir"].Value
        if path = ""
            path := this.app.config.Get("model_dir")
        if !DirExist(path)
            DirCreate(path)
        Run(path)
    }

    HandleEvent(event) {
        switch event["kind"] {
            case "model_test_complete": this.SetStatus(event["text"])
            case "model_download_progress":
                if event["values"].Has("downloaded")
                    this.SetStatus("Downloading " . event["values"].Get("filename", "model") . ": " . event["values"]["downloaded"] . " bytes")
            case "model_download_complete": this.SetStatus("Model download complete: " . event["values"].Get("filename", ""))
        }
    }

    SetStatus(text) {
        if this.controls.Has("status")
            this.controls["status"].Text := text
    }
}
