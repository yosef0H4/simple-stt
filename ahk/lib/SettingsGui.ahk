class SettingsGui {
    __New(app) {
        this.app := app
        this.gui := ""
        this.controls := Map()
        this.buttons := Map()
        this.recorder := HotkeyRecorder(app.logger)
        this.minWidth := 780
        this.minHeight := 650
    }

    Open(*) {
        if IsObject(this.gui) {
            this.gui.Show()
            this.ListInputs()
            this.ListModels()
            return
        }

        window := Gui("+Resize +MinSize780x650", "SimpleStt Settings")
        this.gui := window
        window.BackColor := "F8FAFC"
        window.SetFont("s9", "Segoe UI")
        window.OnEvent("Close", ObjBindMethod(this, "Hide"))
        window.OnEvent("Size", ObjBindMethod(this, "OnSize"))

        window.SetFont("s17 bold c172033", "Segoe UI Variable Display")
        this.controls["title"] := window.AddText("x18 y14 w540 h30", "🎙  SimpleStt")
        window.SetFont("s9 norm c64748B", "Segoe UI Variable Text")
        this.controls["subtitle"] := window.AddText("x20 y48 w820 h30", "Fast local dictation with a calmer setup experience. Tune only what you need, then save once.")
        this.controls["header_line"] := window.AddText("x18 y80 w850 h1 0x10")

        window.SetFont("s9 norm c1F2937", "Segoe UI Variable Text")
        tabs := window.AddTab3("x18 y94 w850 h520 Buttons", ["⌨  General", "🎙  Audio", "✨  Output", "⚙  Advanced"])
        this.controls["tabs"] := tabs

        tabs.UseTab("⌨  General")
        this.controls["general_hotkey_box"] := window.AddText("x34 y138 w810 h164 BackgroundF8FAFC Border")
        window.SetFont("s10 bold c334155", "Segoe UI Variable Text")
        this.controls["general_hotkey_box_title"] := window.AddText("x52 y150 w300 h22", "⌨  Recording shortcut")
        window.SetFont("s9 norm c1F2937", "Segoe UI Variable Text")
        this.controls["hotkey_enabled"] := window.AddCheckbox("x52 y168 w220", "Enable hold-to-record hotkey")
        this.controls["record_hotkey_label"] := window.AddText("x52 y206 w150", "Keyboard shortcut")
        this.controls["record_hotkey"] := window.AddEdit("x205 y201 w310 h25")
        this.AddButton(window, "record_chord", "Record shortcut", "x526 y200 w130 h27", "CaptureHotkey")
        this.controls["capslock_behavior_label"] := window.AddText("x52 y245 w150", "Caps Lock behavior")
        this.controls["capslock_behavior"] := window.AddDropDownList("x205 y240 w220", ["preserve_tap", "always_off"])
        this.controls["hotkey_hint"] := window.AddText("x440 y242 w380 h34 c64748B", "Preserve tap keeps a quick Caps Lock press working normally.")

        this.controls["general_startup_box"] := window.AddText("x34 y320 w810 h132 BackgroundF8FAFC Border")
        window.SetFont("s10 bold c334155", "Segoe UI Variable Text")
        this.controls["general_startup_box_title"] := window.AddText("x52 y332 w300 h22", "🚀  Startup")
        window.SetFont("s9 norm c1F2937", "Segoe UI Variable Text")
        this.controls["start_with_windows"] := window.AddCheckbox("x52 y352 w320", "Start the SimpleStt shell when Windows starts")
        this.controls["startup_hint"] := window.AddText("x52 y384 w720 h38 c64748B", "SimpleStt remains in the tray and only records while the shortcut is held.")
        this.controls["general_tips_box"] := window.AddText("x34 y470 w810 h108 BackgroundF8FAFC Border")
        window.SetFont("s10 bold c334155", "Segoe UI Variable Text")
        this.controls["general_tips_box_title"] := window.AddText("x52 y482 w300 h22", "💡  Quick tips")
        window.SetFont("s9 norm c1F2937", "Segoe UI Variable Text")
        this.controls["general_tips"] := window.AddText("x52 y500 w748 h54 c64748B", "Tap Caps Lock quickly to preserve its normal behavior.`nHold the shortcut only while speaking.`nUse the tray icon to reopen settings anytime.")

        tabs.UseTab("🎙  Audio")
        this.controls["audio_box"] := window.AddText("x34 y138 w810 h186 BackgroundF8FAFC Border")
        window.SetFont("s10 bold c334155", "Segoe UI Variable Text")
        this.controls["audio_box_title"] := window.AddText("x52 y150 w300 h22", "Microphone")
        window.SetFont("s9 norm c1F2937", "Segoe UI Variable Text")
        this.controls["audio_device_contains_label"] := window.AddText("x52 y169 w156", "Device name contains")
        this.controls["audio_device_contains"] := window.AddEdit("x210 y164 w390 h25")
        this.AddButton(window, "list_microphones", "Refresh devices", "x612 y163 w126 h27", "ListInputs")
        this.controls["input_list"] := window.AddDropDownList("x52 y204 w686", ["Use default microphone"])
        this.controls["input_list"].OnEvent("Change", ObjBindMethod(this, "ChooseInput"))
        this.controls["audio_gain_label"] := window.AddText("x52 y247 w156", "Input gain")
        this.controls["audio_gain"] := window.AddEdit("x210 y242 w110 h25")
        this.controls["audio_hint"] := window.AddText("x336 y244 w440 h34 c64748B", "Use 1.0 for normal volume. Adjust only when recordings are consistently quiet or loud.")

        this.controls["model_box"] := window.AddText("x34 y342 w810 h214 BackgroundF8FAFC Border")
        window.SetFont("s10 bold c334155", "Segoe UI Variable Text")
        this.controls["model_box_title"] := window.AddText("x52 y354 w300 h22", "Speech model")
        window.SetFont("s9 norm c1F2937", "Segoe UI Variable Text")
        this.controls["selected_model_filename_label"] := window.AddText("x52 y374 w156", "Selected model")
        this.controls["selected_model_filename"] := window.AddEdit("x210 y369 w390 h25")
        this.AddButton(window, "refresh_models", "Refresh catalog", "x612 y368 w126 h27", "RefreshModels")
        this.controls["model_list"] := window.AddDropDownList("x52 y409 w686", ["Load approved model list"])
        this.controls["model_list"].OnEvent("Change", ObjBindMethod(this, "ChooseModel"))
        this.AddButton(window, "download_model", "Download model", "x52 y453 w140 h29", "DownloadModel")
        this.AddButton(window, "test_model", "Run model test", "x202 y453 w140 h29", "TestModel")
        this.controls["model_hint"] := window.AddText("x52 y496 w720 h34 c64748B", "Models are downloaded locally. The speech worker reloads automatically after a model change.")

        tabs.UseTab("✨  Output")
        this.controls["delivery_box"] := window.AddText("x34 y138 w810 h220 BackgroundF8FAFC Border")
        window.SetFont("s10 bold c334155", "Segoe UI Variable Text")
        this.controls["delivery_box_title"] := window.AddText("x52 y150 w300 h22", "✨  Transcript delivery")
        window.SetFont("s9 norm c1F2937", "Segoe UI Variable Text")
        this.controls["text_delivery_mode_label"] := window.AddText("x52 y170 w158", "Delivery method")
        this.controls["text_delivery_mode"] := window.AddDropDownList("x212 y165 w250", ["type", "paste_ctrl_v", "paste_ctrl_shift_v"])
        this.controls["typing_chunk_chars_label"] := window.AddText("x52 y211 w158", "Characters per chunk")
        this.controls["typing_chunk_chars"] := window.AddEdit("x212 y206 w110 h25")
        this.controls["typing_interval_ms_label"] := window.AddText("x352 y211 w150", "Interval, milliseconds")
        this.controls["typing_interval_ms"] := window.AddEdit("x512 y206 w110 h25")
        this.controls["trailing_space"] := window.AddCheckbox("x52 y252 w220", "Append a trailing space")
        this.controls["remove_punctuation"] := window.AddCheckbox("x52 y286 w220", "Remove punctuation marks")
        this.controls["lowercase_output"] := window.AddCheckbox("x292 y286 w250", "Convert output to lowercase")
        this.controls["delivery_hint"] := window.AddText("x52 y318 w720 h30 c64748B", "Clipboard paste is the fastest option. Simulated typing is useful for applications that block paste.")

        this.controls["worker_box"] := window.AddText("x34 y378 w810 h142 BackgroundF8FAFC Border")
        window.SetFont("s10 bold c334155", "Segoe UI Variable Text")
        this.controls["worker_box_title"] := window.AddText("x52 y390 w300 h22", "⏱  Speech worker")
        window.SetFont("s9 norm c1F2937", "Segoe UI Variable Text")
        this.controls["idle_worker_timeout_secs_label"] := window.AddText("x52 y411 w180", "Idle timeout, seconds")
        this.controls["idle_worker_timeout_secs"] := window.AddEdit("x234 y406 w110 h25")
        this.controls["worker_shutdown_grace_ms_label"] := window.AddText("x382 y411 w180", "Shutdown grace, milliseconds")
        this.controls["worker_shutdown_grace_ms"] := window.AddEdit("x566 y406 w110 h25")
        this.controls["worker_hint"] := window.AddText("x52 y452 w720 h38 c64748B", "The worker is recycled automatically after model changes or after the idle timeout.")

        tabs.UseTab("⚙  Advanced")
        this.controls["logging_box"] := window.AddText("x34 y138 w810 h150 BackgroundF8FAFC Border")
        window.SetFont("s10 bold c334155", "Segoe UI Variable Text")
        this.controls["logging_box_title"] := window.AddText("x52 y150 w300 h22", "🔧  Diagnostics")
        window.SetFont("s9 norm c1F2937", "Segoe UI Variable Text")
        this.controls["log_level_label"] := window.AddText("x52 y171 w156", "Logging level")
        this.controls["log_level"] := window.AddDropDownList("x210 y166 w180", ["minimal", "normal", "debug", "extreme"])
        this.controls["diagnostic_overlay"] := window.AddCheckbox("x52 y211 w230", "Show diagnostic overlay notices")
        this.controls["log_transcripts"] := window.AddCheckbox("x302 y211 w260", "Write transcripts to the diagnostic log")
        this.controls["logging_hint"] := window.AddText("x52 y246 w720 h28 c64748B", "Leave diagnostics off for normal use. Enable them temporarily while troubleshooting.")

        this.controls["paths_box"] := window.AddText("x34 y306 w810 h218 BackgroundF8FAFC Border")
        window.SetFont("s10 bold c334155", "Segoe UI Variable Text")
        this.controls["paths_box_title"] := window.AddText("x52 y318 w300 h22", "📁  Runtime locations")
        window.SetFont("s9 norm c1F2937", "Segoe UI Variable Text")
        this.controls["parakeet_runtime_dir_label"] := window.AddText("x52 y340 w156", "Runtime directory")
        this.controls["parakeet_runtime_dir"] := window.AddEdit("x210 y335 w570 h25")
        this.controls["model_dir_label"] := window.AddText("x52 y382 w156", "Model directory")
        this.controls["model_dir"] := window.AddEdit("x210 y377 w420 h25")
        this.AddButton(window, "browse_models", "Browse", "x642 y376 w80 h27", "BrowseModelDir")
        this.AddButton(window, "open_models", "Open folder", "x732 y376 w100 h27", "OpenModelDir")
        this.controls["paths_hint"] := window.AddText("x52 y425 w720 h48 c64748B", "Relative paths are resolved from the SimpleStt runtime folder. Most users should keep the packaged defaults.")

        tabs.UseTab()
        this.controls["footer_line"] := window.AddText("x18 y624 w850 h1 0x10")
        this.controls["status"] := window.AddText("x20 y640 w500 h25 c475569", "✓ Ready")
        this.AddButton(window, "save", "✓ Save changes", "x560 y635 w124 h30 Default", "Save")
        this.AddButton(window, "reload", "↻ Reload", "x694 y635 w92 h30", "Reload")
        this.AddButton(window, "open_log", "📄 Open log", "x796 y635 w102 h30", "OpenLatestLog", true)

        this.LoadControls()
        window.Show("w900 h700")
        this.Layout(900, 700)
        this.ListInputs()
        this.ListModels()
    }

    AddButton(window, key, label, options, method, appMethod := false) {
        button := window.AddButton(options, label)
        callback := appMethod ? ObjBindMethod(this.app, method) : ObjBindMethod(this, method)
        button.OnEvent("Click", callback)
        this.controls[key] := button
        this.buttons[key] := button
        return button
    }

    OnSize(guiObj, minMax, width, height) {
        if minMax = -1
            return
        this.Layout(width, height)
    }

    Layout(width, height) {
        if !IsObject(this.gui)
            return
        width := Max(width, this.minWidth)
        height := Max(height, this.minHeight)
        margin := 18
        inner := width - (margin * 2)
        pageW := Min(inner, 1040)
        pageX := Floor((width - pageW) / 2)
        tabTop := 94
        footerHeight := 72
        tabHeight := height - tabTop - footerHeight - 8
        contentX := pageX + 16
        contentW := pageW - 32
        right := contentX + contentW
        fieldX := contentX + 176
        buttonW := 126

        this.controls["title"].Move(pageX, 14, pageW - 12, 30)
        this.controls["subtitle"].Move(pageX + 2, 48, pageW - 12, 30)
        this.controls["header_line"].Move(pageX, 80, pageW, 1)
        this.controls["tabs"].Move(pageX, tabTop, pageW, tabHeight)
        footerY := height - footerHeight + 8
        this.controls["footer_line"].Move(margin, footerY, inner, 1)
        this.controls["status"].Move(20, footerY + 17, Max(250, width - 430), 25)
        this.controls["save"].Move(width - 362, footerY + 11, 124, 30)
        this.controls["reload"].Move(width - 228, footerY + 11, 92, 30)
        this.controls["open_log"].Move(width - 126, footerY + 11, 108, 30)

        this.controls["general_hotkey_box"].Move(contentX, 138, contentW, 164)
        this.controls["general_hotkey_box_title"].Move(contentX + 18, 150, contentW - 36, 22)
        this.controls["record_hotkey"].Move(fieldX, 201, Max(220, contentW - 468), 25)
        this.controls["record_chord"].Move(right - 150, 200, 132, 27)
        this.controls["hotkey_hint"].Move(fieldX + 235, 242, Max(210, contentW - 420), 34)
        this.controls["general_startup_box"].Move(contentX, 320, contentW, 132)
        this.controls["general_startup_box_title"].Move(contentX + 18, 332, contentW - 36, 22)
        this.controls["startup_hint"].Move(contentX + 18, 384, contentW - 36, 38)
        this.controls["general_tips_box"].Move(contentX, 470, contentW, 108)
        this.controls["general_tips_box_title"].Move(contentX + 18, 482, contentW - 36, 22)
        this.controls["general_tips"].Move(contentX + 18, 500, contentW - 36, 54)

        this.controls["audio_box"].Move(contentX, 138, contentW, 186)
        this.controls["audio_box_title"].Move(contentX + 18, 150, contentW - 36, 22)
        this.controls["audio_device_contains"].Move(fieldX, 164, Max(210, contentW - 366), 25)
        this.controls["list_microphones"].Move(right - 144, 163, buttonW, 27)
        this.controls["input_list"].Move(contentX + 18, 204, contentW - 36, 25)
        this.controls["audio_hint"].Move(fieldX + 126, 244, Max(240, contentW - 320), 34)
        this.controls["model_box"].Move(contentX, 342, contentW, 214)
        this.controls["model_box_title"].Move(contentX + 18, 354, contentW - 36, 22)
        this.controls["selected_model_filename"].Move(fieldX, 369, Max(210, contentW - 366), 25)
        this.controls["refresh_models"].Move(right - 144, 368, buttonW, 27)
        this.controls["model_list"].Move(contentX + 18, 409, contentW - 36, 25)
        this.controls["model_hint"].Move(contentX + 18, 496, contentW - 36, 34)

        this.controls["delivery_box"].Move(contentX, 138, contentW, 220)
        this.controls["delivery_box_title"].Move(contentX + 18, 150, contentW - 36, 22)
        this.controls["delivery_hint"].Move(contentX + 18, 318, contentW - 36, 30)
        this.controls["worker_box"].Move(contentX, 378, contentW, 142)
        this.controls["worker_box_title"].Move(contentX + 18, 390, contentW - 36, 22)
        this.controls["worker_hint"].Move(contentX + 18, 452, contentW - 36, 38)

        this.controls["logging_box"].Move(contentX, 138, contentW, 150)
        this.controls["logging_box_title"].Move(contentX + 18, 150, contentW - 36, 22)
        this.controls["logging_hint"].Move(contentX + 18, 246, contentW - 36, 28)
        this.controls["paths_box"].Move(contentX, 306, contentW, 218)
        this.controls["paths_box_title"].Move(contentX + 18, 318, contentW - 36, 22)
        this.controls["parakeet_runtime_dir"].Move(fieldX, 335, contentW - 212, 25)
        this.controls["model_dir"].Move(fieldX, 377, Max(180, contentW - 372), 25)
        this.controls["browse_models"].Move(right - 190, 376, 80, 27)
        this.controls["open_models"].Move(right - 100, 376, 82, 27)
        this.controls["paths_hint"].Move(contentX + 18, 425, contentW - 36, 48)
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
        if this.app.HasProp("testMode") && this.app.testMode {
            this.SetStatus("Preview: shortcut recorder opened safely")
            return
        }
        this.SetStatus("Hold the desired modifiers, then press the final key.")
        this.recorder.Start(ObjBindMethod(this, "HotkeyCaptured"))
    }

    HotkeyCaptured(label) {
        this.controls["record_hotkey"].Value := label
        this.SetStatus("Recorded " . label . ". Press Save to apply it.")
    }

    ListInputs(*) {
        this.SetStatus("Loading microphones...")
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
        this.SetStatus("Loading cached models...")
        this.app.ipc.CallService("list-models", ObjBindMethod(this, "ModelsLoaded"))
    }

    RefreshModels(*) {
        this.SetStatus("Refreshing online model catalog...")
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
        if this.app.HasProp("testMode") && this.app.testMode {
            this.SetStatus("Preview: folder picker opened safely")
            return
        }
        current := this.controls["model_dir"].Value
        selected := DirSelect(current, 1, "Choose the SimpleStt model folder")
        if selected != ""
            this.controls["model_dir"].Value := selected
    }

    OpenModelDir(*) {
        if this.app.HasProp("testMode") && this.app.testMode {
            this.SetStatus("Preview: model folder opened safely")
            return
        }
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

    TestAllButtons(*) {
        if !(this.app.HasProp("testMode") && this.app.testMode)
            throw Error("Button exercise is available only in test mode")
        this.CaptureHotkey()
        this.ListInputs()
        this.RefreshModels()
        this.DownloadModel()
        this.TestModel()
        this.BrowseModelDir()
        this.OpenModelDir()
        this.Save()
        this.Reload()
        this.app.OpenLatestLog()
        this.SetStatus("Preview: all buttons exercised safely")
    }
}
