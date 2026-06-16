; SettingsNav is a thin adapter so the preview/smoke harnesses can keep driving
; page switches through controls["tabs"].Choose(index) after the tab strip was
; replaced with a custom left sidebar.
class SettingsNav {
    __New(owner) {
        this.owner := owner
    }

    Choose(index) {
        this.owner.ShowPage(index)
    }

    Value {
        get => this.owner.activePage
    }
}

class SettingsGui {
    __New(app) {
        this.app := app
        this.gui := ""
        this.controls := Map()
        this.buttons := Map()
        this.buttonCallbacks := Map()
        this.installedModelChoices := Map()
        this.catalogModelChoices := Map()
        this.recorder := HotkeyRecorder(app.logger)
        this.minWidth := 900
        this.minHeight := 740
        this.defaultWidth := 1040
        this.defaultHeight := 740
        this.dark := false
        this.themeMode := "auto"
        this.col := Map()
        this.pages := Map()
        this.navButtons := []
        this.activePage := 1
        this.loadingControls := false
    }

    Open(refreshLists := true, *) {
        if IsObject(this.gui) {
            this.gui.Show()
            if refreshLists {
                this.ListInputs()
                this.ListModels()
            }
            return
        }

        this.controls := Map()
        this.buttons := Map()
        this.buttonCallbacks := Map()
        this.pages := Map()
        this.navButtons := []
        this.activePage := 1
        this.themeMode := this.app.config.Get("ui_theme", "auto")
        this.dark := this.ResolveDark()
        this.BuildPalette()

        window := Gui("+Resize +MinSize" . this.minWidth . "x" . this.minHeight, "SimpleStt Settings")
        this.gui := window
        window.BackColor := this.col["win"]
        window.SetFont("s9", "Segoe UI")
        window.OnEvent("Close", ObjBindMethod(this, "Hide"))
        window.OnEvent("Size", ObjBindMethod(this, "OnSize"))

        ; ---- sidebar shell ----
        this.controls["sidebar"] := window.AddText("x0 y0 w204 h720 Background" . this.col["sidebar"], "")
        this.controls["sidebar_divider"] := window.AddText("x203 y0 w1 h720 Background" . this.col["border"], "")
        window.SetFont("s14 bold c" . this.col["text"], "Segoe UI Variable Display")
        this.controls["brand"] := window.AddText("x20 y22 w170 h30 +0x80 Background" . this.col["sidebar"], "🎙  SimpleStt")
        window.SetFont("s8 norm c" . this.col["subtext"], "Segoe UI")
        this.controls["brand_sub"] := window.AddText("x22 y54 w168 h20 +0x80 Background" . this.col["sidebar"], "Local dictation settings")

        this.controls["nav_accent"] := window.AddText("x0 y112 w4 h40 Background" . this.col["accent"], "")
        navLabels := ["   ⌨  General", "   🎙  Audio & models", "   ✨  Output", "   ⚙  Advanced"]
        for i, label in navLabels {
            window.SetFont("s10 norm c" . this.col["navText"], "Segoe UI")
            item := window.AddText("x4 y" . (112 + (i - 1) * 46) . " w196 h40 +0x200 +0x80 +0x100 Background" . this.col["sidebar"], label)
            item.OnEvent("Click", this.NavHandler(i))
            this.controls["nav" . i] := item
            this.navButtons.Push(item)
        }

        ; ---- content header ----
        window.SetFont("s15 bold c" . this.col["text"], "Segoe UI Variable Display")
        this.controls["page_title"] := window.AddText("x228 y22 w600 h30 +0x80 Background" . this.col["win"], "General")
        window.SetFont("s9 norm c" . this.col["subtext"], "Segoe UI")
        this.controls["page_sub"] := window.AddText("x228 y56 w600 h22 +0x80 Background" . this.col["win"], "")

        ; ---- page 1: General ----
        this.Panel("general_hotkey_box", 1)
        this.PTitle("general_hotkey_box_title", 1, "⌨  Recording shortcut")
        this.MkCheck("hotkey_enabled", 1, "Enable hold-to-record hotkey")
        this.Field("record_hotkey_label", 1, "Keyboard shortcut")
        this.MkDisplay("record_hotkey", 1)
        this.AddButton("record_chord", "Record shortcut", "CaptureHotkey", false, 1)
        this.Field("cancel_hotkey_label", 1, "Cancel shortcut")
        this.MkDisplay("cancel_hotkey", 1)
        this.AddButton("record_cancel_chord", "Record cancel", "CaptureCancelHotkey", false, 1)
        this.Field("toggle_delivery_hotkey_label", 1, "Toggle typing/paste")
        this.MkDisplay("toggle_delivery_hotkey", 1)
        this.AddButton("record_toggle_chord", "Record toggle", "CaptureToggleHotkey", false, 1)
        this.Field("capslock_behavior_label", 1, "Caps Lock behavior")
        this.MkDrop("capslock_behavior", 1, ["preserve_tap", "always_off"])
        this.Hint("hotkey_hint", 1, "Preserve tap keeps a quick Caps Lock press working normally.`nUse the toggle hotkey to switch between typing and paste.")

        this.Panel("general_startup_box", 1)
        this.PTitle("general_startup_box_title", 1, "🚀  Startup")
        this.MkCheck("start_with_windows", 1, "Start the SimpleStt shell when Windows starts")
        this.Hint("startup_hint", 1, "SimpleStt remains in the tray and only records while the shortcut is held.")

        this.Panel("general_appearance_box", 1)
        this.PTitle("general_appearance_box_title", 1, "🎨  Appearance")
        this.Field("ui_theme_label", 1, "Theme")
        this.MkDrop("ui_theme", 1, ["auto", "light", "dark"])
        this.controls["ui_theme"].OnEvent("Change", ObjBindMethod(this, "ThemeChanged"))
        this.Hint("ui_theme_hint", 1, "Preview updates immediately. Press Save changes to keep the theme.")


        ; ---- page 2: Audio & models ----
        this.Panel("audio_box", 2)
        this.PTitle("audio_box_title", 2, "🎙  Microphone")
        this.Field("audio_device_contains_label", 2, "Microphone")
        this.MkDrop("audio_device_contains", 2, ["Default microphone"])
        this.controls["audio_device_contains"].OnEvent("Change", ObjBindMethod(this, "ChooseInput"))
        this.AddButton("list_microphones", "Refresh devices", "ListInputs", false, 2)
        this.Field("audio_gain_label", 2, "Input gain")
        this.MkEdit("audio_gain", 2)
        this.Hint("audio_hint", 2, "Pick a device directly. Use 1.0 for normal volume unless recordings are consistently quiet or loud.")

        this.Panel("model_box", 2)
        this.PTitle("model_box_title", 2, "🧠  Speech model")
        this.Field("selected_model_filename_label", 2, "Installed model")
        this.MkDrop("selected_model_filename", 2, ["No installed models found"])
        this.controls["selected_model_filename"].OnEvent("Change", ObjBindMethod(this, "ChooseInstalledModel"))
        this.AddButton("refresh_models", "Refresh catalog", "RefreshModels", false, 2)
        this.Field("model_list_label", 2, "Download catalog")
        this.MkDrop("model_list", 2, ["Load approved model list"])
        this.controls["model_list"].OnEvent("Change", ObjBindMethod(this, "ChooseCatalogModel"))
        this.AddButton("download_model", "Download model", "DownloadModel", false, 2)
        this.AddButton("test_model", "Run model test", "TestModel", false, 2)
        this.Hint("model_hint", 2, "Recommended: NVIDIA GPU -> tdt_ctc-110m-f16.gguf. CPU/no GPU -> tdt_ctc-110m-q4_k.gguf.")

        ; ---- page 3: Output ----
        this.Panel("delivery_box", 3)
        this.PTitle("delivery_box_title", 3, "✨  Transcript delivery")
        this.Field("text_delivery_mode_label", 3, "Delivery method")
        this.MkDrop("text_delivery_mode", 3, ["type", "paste_ctrl_v", "paste_ctrl_shift_v"])
        this.Field("typing_chunk_chars_label", 3, "Characters per chunk")
        this.MkEdit("typing_chunk_chars", 3)
        this.Field("typing_interval_ms_label", 3, "Interval, milliseconds")
        this.MkEdit("typing_interval_ms", 3)
        this.MkCheck("trailing_space", 3, "Append a trailing space")
        this.MkCheck("remove_punctuation", 3, "Remove punctuation marks")
        this.MkCheck("lowercase_output", 3, "Convert output to lowercase")
        this.Hint("delivery_hint", 3, "Clipboard paste is the fastest option. Simulated typing is useful for applications that block paste.`nThe toggle hotkey can switch between typing and paste instantly.")

        this.Panel("worker_box", 3)
        this.PTitle("worker_box_title", 3, "⏱  Speech worker")
        this.Field("idle_worker_timeout_secs_label", 3, "Idle timeout, seconds")
        this.MkEdit("idle_worker_timeout_secs", 3)
        this.Field("worker_shutdown_grace_ms_label", 3, "Shutdown grace, milliseconds")
        this.MkEdit("worker_shutdown_grace_ms", 3)
        this.Hint("worker_hint", 3, "The worker is recycled automatically after model changes or after the idle timeout.")

        ; ---- page 4: Advanced ----
        this.Panel("logging_box", 4)
        this.PTitle("logging_box_title", 4, "🔧  Diagnostics")
        this.Field("log_level_label", 4, "Logging level")
        this.MkDrop("log_level", 4, ["minimal", "normal", "debug", "extreme"])
        this.MkCheck("diagnostic_overlay", 4, "Show diagnostic overlay notices")
        this.MkCheck("log_transcripts", 4, "Write transcripts to the diagnostic log")
        this.Hint("logging_hint", 4, "Leave diagnostics off for normal use. Enable them temporarily while troubleshooting.")

        this.Panel("device_box", 4)
        this.PTitle("device_box_title", 4, "⚡  Inference device")
        this.Field("inference_device_label", 4, "Compute backend")
        this.MkDrop("inference_device", 4, ["auto", "nvidia_gpu", "cpu"])
        this.controls["inference_device"].OnEvent("Change", ObjBindMethod(this, "InferenceDeviceChanged"))
        this.Hint("device_hint", 4, "Auto uses NVIDIA GPU when available, otherwise CPU.`nModel recommendations update when this changes.")

        this.Panel("paths_box", 4)
        this.PTitle("paths_box_title", 4, "📁  Runtime locations")
        this.Field("parakeet_runtime_dir_label", 4, "Runtime directory")
        this.MkEdit("parakeet_runtime_dir", 4)
        this.Field("model_dir_label", 4, "Model directory")
        this.MkEdit("model_dir", 4)
        this.AddButton("browse_models", "Browse", "BrowseModelDir", false, 4)
        this.AddButton("open_models", "Open folder", "OpenModelDir", false, 4)
        this.Hint("paths_hint", 4, "Paths here are shown as absolute locations so portable, installed, and dev builds do not accidentally share the wrong runtime or model folder.")

        ; ---- footer ----
        this.controls["footer_line"] := window.AddText("x0 y0 w100 h1 Background" . this.col["border"], "")
        window.SetFont("s9 norm c" . this.col["subtext"], "Segoe UI")
        this.controls["status"] := window.AddText("x0 y0 w400 h22 +0x80 Background" . this.col["win"], "✓ Ready")
        this.AddButton("save", "✓  Save changes", "Save", false, 0, "accent")
        this.AddButton("reload", "↻  Reload", "Reload", false, 0, "normal")
        this.AddButton("open_log", "📄  Open log", "OpenLatestLog", true, 0, "normal")

        this.controls["tabs"] := SettingsNav(this)

        this.ApplyTitleBar()
        this.loadingControls := true
        this.LoadControls()
        this.loadingControls := false
        this.Layout(this.defaultWidth, this.defaultHeight)
        this.ShowPage(1)
        window.Show("w" . this.defaultWidth . " h" . this.defaultHeight)
        this.RefreshVisibleControls()
        if refreshLists {
            this.ListInputs()
            this.ListModels()
        }
    }

    ; ---- theme detection and palette ----

    ResolveDark() {
        if this.themeMode = "dark"
            return true
        if this.themeMode = "light"
            return false
        try {
            value := RegRead("HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize", "AppsUseLightTheme")
            return value = 0
        }
        return false
    }

    BuildPalette() {
        if this.dark {
            this.col := Map(
                "win", "1B1B1F",
                "sidebar", "202024",
                "card", "28282E",
                "border", "3A3A42",
                "text", "ECECF1",
                "subtext", "9A9AA5",
                "accent", "3B82F6",
                "navText", "C2C2CC",
                "navActiveBg", "31313A",
                "input", "2E2E36")
        } else {
            this.col := Map(
                "win", "F3F5F8",
                "sidebar", "FFFFFF",
                "card", "FFFFFF",
                "border", "E2E8F0",
                "text", "1F2933",
                "subtext", "64748B",
                "accent", "2563EB",
                "navText", "475569",
                "navActiveBg", "EAF1FB",
                "input", "FFFFFF")
        }
    }

    ; ---- Windows theming hooks ----

    ApplyTitleBar() {
        hwnd := this.gui.Hwnd
        buf := Buffer(4, 0)
        NumPut("int", this.dark ? 1 : 0, buf)
        for attr in [20, 19] {
            try DllCall("dwmapi\DwmSetWindowAttribute", "ptr", hwnd, "int", attr, "ptr", buf, "int", 4)
        }
    }

    ThemeCtrl(ctrl, darkTheme) {
        theme := this.dark ? darkTheme : "Explorer"
        try DllCall("uxtheme\SetWindowTheme", "ptr", ctrl.Hwnd, "wstr", theme, "ptr", 0)
    }

    ; ---- control factories (positions are assigned later in Layout) ----

    Reg(key, page, ctrl) {
        this.controls[key] := ctrl
        this.RegPage(key, page)
    }

    RegPage(key, page) {
        if page = 0
            return
        if !this.pages.Has(page)
            this.pages[page] := []
        this.pages[page].Push(key)
    }

    Panel(key, page) {
        opt := "x0 y0 w300 h100 Background" . this.col["card"] . (this.dark ? "" : " Border")
        ctrl := this.gui.AddText(opt, "")
        this.Reg(key, page, ctrl)
        return ctrl
    }

    PTitle(key, page, text) {
        this.gui.SetFont("s10 bold c" . this.col["text"], "Segoe UI Variable Display")
        ctrl := this.gui.AddText("x0 y0 w300 h22 +0x80 Background" . this.col["card"], text)
        this.Reg(key, page, ctrl)
        return ctrl
    }

    Field(key, page, text) {
        this.gui.SetFont("s9 norm c" . this.col["text"], "Segoe UI")
        ctrl := this.gui.AddText("x0 y0 w150 h22 Background" . this.col["card"], text)
        this.Reg(key, page, ctrl)
        return ctrl
    }

    Hint(key, page, text) {
        this.gui.SetFont("s9 norm c" . this.col["subtext"], "Segoe UI")
        ctrl := this.gui.AddText("x0 y0 w300 h30 Background" . this.col["card"], text)
        this.Reg(key, page, ctrl)
        return ctrl
    }

    MkEdit(key, page) {
        this.gui.SetFont("s9 norm c" . this.col["text"], "Segoe UI")
        ; Use a flat, thin bordered edit field in both themes so text inputs
        ; read as fields instead of blending into the card background. ES_CENTER
        ; keeps short values visually balanced inside wide fields.
        opts := "x0 y0 w120 h25 -E0x200 +0x800000 +0x1 Background" . this.col["input"]
        ctrl := this.gui.AddEdit(opts)
        this.ThemeCtrl(ctrl, "DarkMode_Explorer")
        this.Reg(key, page, ctrl)
        return ctrl
    }

    MkDisplay(key, page) {
        this.gui.SetFont("s9 norm c" . this.col["text"], "Segoe UI")
        ; Hotkey fields are changed through the recorder buttons. Text controls
        ; allow true horizontal and vertical centering, unlike native Edit.
        ctrl := this.gui.AddText("x0 y0 w120 h25 +0x200 Center Border Background" . this.col["input"], "")
        this.Reg(key, page, ctrl)
        return ctrl
    }

    MkDrop(key, page, items) {
        this.gui.SetFont("s9 norm c" . this.col["text"], "Segoe UI")
        ctrl := this.gui.AddDropDownList("x0 y0 w200 Background" . this.col["input"], items)
        this.ThemeCtrl(ctrl, "DarkMode_CFD")
        this.Reg(key, page, ctrl)
        return ctrl
    }

    MkCheck(key, page, text) {
        this.gui.SetFont("s9 norm c" . this.col["text"], "Segoe UI")
        ctrl := this.gui.AddCheckbox("x0 y0 w260 h22 Background" . this.col["card"], text)
        this.ThemeCtrl(ctrl, "DarkMode_Explorer")
        this.Reg(key, page, ctrl)
        return ctrl
    }

    AddButton(key, label, method, appMethod := false, page := 0, style := "normal") {
        callback := appMethod ? ObjBindMethod(this.app, method) : ObjBindMethod(this, method)
        this.buttonCallbacks[key] := callback
        this.gui.SetFont("s9 norm c" . this.col["text"], "Segoe UI")
        ; Native Win32 buttons do not theme cleanly in dark mode and look too
        ; different between themes. Use one flat clickable Text-button style in
        ; both light and dark modes, with a thin border so actions remain clear.
        bg := style = "accent" ? this.col["input"] : this.col["input"]
        button := this.gui.AddText("x0 y0 w120 h28 +0x100 +0x200 Center Border Background" . bg, label)
        button.OnEvent("Click", callback)
        this.controls[key] := button
        this.buttons[key] := button
        this.RegPage(key, page)
        return button
    }

    ; ---- page switching ----

    RefreshVisibleControls() {
        for _, ctrl in this.controls {
            if IsObject(ctrl) && ctrl.HasProp("Hwnd")
                try DllCall("user32\InvalidateRect", "ptr", ctrl.Hwnd, "ptr", 0, "int", 1)
        }
        try DllCall("user32\RedrawWindow", "ptr", this.gui.Hwnd, "ptr", 0, "ptr", 0, "uint", 0x0101)
    }

    ; Returns a click handler bound to a specific page index. A fat-arrow
    ; closure created directly in the build loop would capture the loop
    ; variable by reference, so every nav item would share the final value
    ; (Advanced). Binding through a parameter gives each handler its own index.
    NavHandler(index) {
        return (*) => this.ShowPage(index)
    }

    ShowPage(index) {
        if !IsObject(this.gui)
            return
        if index < 1 || index > 4
            index := 1
        this.activePage := index
        titles := ["General", "Audio & models", "Output", "Advanced"]
        subs := [
            "Recording shortcut, Caps Lock behavior, and startup.",
            "Microphone input and the speech recognition model.",
            "How transcripts are delivered and the speech worker.",
            "Diagnostics, inference device, and runtime locations."]
        this.controls["page_title"].Text := titles[index]
        this.controls["page_sub"].Text := subs[index]
        for pageNumber, keys in this.pages
            for key in keys
                this.controls[key].Visible := (pageNumber = index)
        for i, btn in this.navButtons {
            active := (i = index)
            btn.Opt("Background" . (active ? this.col["navActiveBg"] : this.col["sidebar"]))
            btn.SetFont("c" . (active ? this.col["text"] : this.col["navText"]))
            DllCall("user32\InvalidateRect", "ptr", btn.Hwnd, "ptr", 0, "int", 1)
        }
        this.controls["nav_accent"].Move(0, 112 + (index - 1) * 46, 4, 40)
        this.RefreshVisibleControls()
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
        sidebarW := 204
        footerH := 64
        footerY := height - footerH
        pad := 24

        this.controls["sidebar"].Move(0, 0, sidebarW, footerY)
        this.controls["sidebar_divider"].Move(sidebarW - 1, 0, 1, footerY)
        this.controls["brand"].Move(20, 22, sidebarW - 34, 30)
        this.controls["brand_sub"].Move(22, 54, sidebarW - 36, 20)
        for i, btn in this.navButtons
            btn.Move(4, 112 + (i - 1) * 46, sidebarW - 8, 40)
        this.controls["nav_accent"].Move(0, 112 + (this.activePage - 1) * 46, 4, 40)

        contentX := sidebarW + pad
        contentW := width - contentX - pad
        right := contentX + contentW
        fieldX := contentX + 176
        ; Second column for two-up rows, scaled to the available width so the
        ; right-hand field always stays inside the card at the minimum size.
        col2X := contentX + Max(330, Floor(contentW / 2))
        col2FieldX := col2X + 184

        this.controls["page_title"].Move(contentX, 22, contentW, 30)
        this.controls["page_sub"].Move(contentX, 56, contentW, 22)

        this.controls["footer_line"].Move(0, footerY, width, 1)
        this.controls["status"].Move(contentX, footerY + 21, Max(200, width - 380 - contentX), 22)
        this.controls["save"].Move(width - 360, footerY + 16, 132, 32)
        this.controls["reload"].Move(width - 214, footerY + 16, 92, 32)
        this.controls["open_log"].Move(width - 112, footerY + 16, 96, 32)

        ; ---- page 1: General ----
        this.controls["general_hotkey_box"].Move(contentX, 96, contentW, 256)
        this.controls["general_hotkey_box_title"].Move(contentX + 18, 108, contentW - 36, 22)
        this.controls["hotkey_enabled"].Move(contentX + 18, 138, 300, 22)
        this.controls["record_hotkey_label"].Move(contentX + 18, 178, 150, 22)
        this.controls["record_hotkey"].Move(fieldX, 174, Max(180, contentW - 360), 25)
        this.controls["record_chord"].Move(right - 150, 173, 132, 27)
        this.controls["cancel_hotkey_label"].Move(contentX + 18, 214, 150, 22)
        this.controls["cancel_hotkey"].Move(fieldX, 210, Max(180, contentW - 360), 25)
        this.controls["record_cancel_chord"].Move(right - 150, 209, 132, 27)
        this.controls["toggle_delivery_hotkey_label"].Move(contentX + 18, 250, 150, 22)
        this.controls["toggle_delivery_hotkey"].Move(fieldX, 246, Max(180, contentW - 360), 25)
        this.controls["record_toggle_chord"].Move(right - 150, 245, 132, 27)
        this.controls["capslock_behavior_label"].Move(contentX + 18, 286, 150, 22)
        this.controls["capslock_behavior"].Move(fieldX, 282, 220, 120)
        this.controls["hotkey_hint"].Move(contentX + 18, 316, contentW - 36, 30)
        this.controls["general_startup_box"].Move(contentX, 372, contentW, 96)
        this.controls["general_startup_box_title"].Move(contentX + 18, 384, contentW - 36, 22)
        this.controls["start_with_windows"].Move(contentX + 18, 412, contentW - 36, 22)
        this.controls["startup_hint"].Move(contentX + 18, 438, contentW - 36, 22)
        this.controls["general_appearance_box"].Move(contentX, 488, contentW, 104)
        this.controls["general_appearance_box_title"].Move(contentX + 18, 500, contentW - 36, 22)
        this.controls["ui_theme_label"].Move(contentX + 18, 532, 150, 22)
        this.controls["ui_theme"].Move(fieldX, 528, 160, 120)
        this.controls["ui_theme_hint"].Move(contentX + 18, 562, contentW - 36, 22)

        ; ---- page 2: Audio & models ----
        this.controls["audio_box"].Move(contentX, 96, contentW, 172)
        this.controls["audio_box_title"].Move(contentX + 18, 108, contentW - 36, 22)
        this.controls["audio_device_contains_label"].Move(contentX + 18, 142, 150, 22)
        this.controls["audio_device_contains"].Move(fieldX, 138, Max(200, contentW - 360), 200)
        this.controls["list_microphones"].Move(right - 150, 137, 132, 27)
        this.controls["audio_gain_label"].Move(contentX + 18, 184, 150, 22)
        this.controls["audio_gain"].Move(fieldX, 180, 110, 25)
        this.controls["audio_hint"].Move(contentX + 18, 214, contentW - 36, 34)
        this.controls["model_box"].Move(contentX, 288, contentW, 224)
        this.controls["model_box_title"].Move(contentX + 18, 300, contentW - 36, 22)
        this.controls["selected_model_filename_label"].Move(contentX + 18, 334, 150, 22)
        this.controls["selected_model_filename"].Move(fieldX, 330, Max(200, contentW - 360), 200)
        this.controls["refresh_models"].Move(right - 150, 329, 132, 27)
        this.controls["model_list_label"].Move(contentX + 18, 374, 150, 22)
        this.controls["model_list"].Move(fieldX, 370, contentW - 208, 200)
        this.controls["download_model"].Move(contentX + 18, 410, 150, 30)
        this.controls["test_model"].Move(contentX + 180, 410, 150, 30)
        this.controls["model_hint"].Move(contentX + 18, 452, contentW - 36, 44)

        ; ---- page 3: Output ----
        this.controls["delivery_box"].Move(contentX, 96, contentW, 236)
        this.controls["delivery_box_title"].Move(contentX + 18, 108, contentW - 36, 22)
        this.controls["text_delivery_mode_label"].Move(contentX + 18, 142, 158, 22)
        this.controls["text_delivery_mode"].Move(fieldX, 138, 250, 120)
        this.controls["typing_chunk_chars_label"].Move(contentX + 18, 182, 158, 22)
        this.controls["typing_chunk_chars"].Move(fieldX, 178, 110, 25)
        this.controls["typing_interval_ms_label"].Move(col2X, 182, 180, 22)
        this.controls["typing_interval_ms"].Move(col2FieldX, 178, 110, 25)
        this.controls["trailing_space"].Move(contentX + 18, 222, contentW - 36, 22)
        this.controls["remove_punctuation"].Move(contentX + 18, 256, col2X - contentX - 18, 22)
        this.controls["lowercase_output"].Move(col2X, 256, right - col2X - 18, 22)
        this.controls["delivery_hint"].Move(contentX + 18, 292, contentW - 36, 34)
        this.controls["worker_box"].Move(contentX, 352, contentW, 150)
        this.controls["worker_box_title"].Move(contentX + 18, 364, contentW - 36, 22)
        this.controls["idle_worker_timeout_secs_label"].Move(contentX + 18, 398, 158, 22)
        this.controls["idle_worker_timeout_secs"].Move(fieldX, 394, 110, 25)
        this.controls["worker_shutdown_grace_ms_label"].Move(col2X, 398, 180, 22)
        this.controls["worker_shutdown_grace_ms"].Move(col2FieldX, 394, 110, 25)
        this.controls["worker_hint"].Move(contentX + 18, 438, contentW - 36, 34)

        ; ---- page 4: Advanced ----
        this.controls["logging_box"].Move(contentX, 96, contentW, 150)
        this.controls["logging_box_title"].Move(contentX + 18, 108, contentW - 36, 22)
        this.controls["log_level_label"].Move(contentX + 18, 142, 156, 22)
        this.controls["log_level"].Move(fieldX, 138, 180, 120)
        this.controls["diagnostic_overlay"].Move(contentX + 18, 182, col2X - contentX - 18, 22)
        this.controls["log_transcripts"].Move(col2X, 182, right - col2X - 18, 22)
        this.controls["logging_hint"].Move(contentX + 18, 216, contentW - 36, 22)
        this.controls["device_box"].Move(contentX, 266, contentW, 124)
        this.controls["device_box_title"].Move(contentX + 18, 278, contentW - 36, 22)
        this.controls["inference_device_label"].Move(contentX + 18, 312, 156, 22)
        this.controls["inference_device"].Move(fieldX, 308, 220, 120)
        this.controls["device_hint"].Move(contentX + 18, 348, contentW - 36, 34)
        this.controls["paths_box"].Move(contentX, 410, contentW, 168)
        this.controls["paths_box_title"].Move(contentX + 18, 422, contentW - 36, 22)
        this.controls["parakeet_runtime_dir_label"].Move(contentX + 18, 452, 156, 22)
        this.controls["parakeet_runtime_dir"].Move(fieldX, 448, contentW - 212, 25)
        this.controls["model_dir_label"].Move(contentX + 18, 492, 156, 22)
        this.controls["model_dir"].Move(fieldX, 488, Max(180, contentW - 392), 25)
        this.controls["browse_models"].Move(right - 190, 487, 80, 27)
        this.controls["open_models"].Move(right - 100, 487, 82, 27)
        this.controls["paths_hint"].Move(contentX + 18, 528, contentW - 36, 40)
    }

    Hide(*) {
        if IsObject(this.gui)
            this.gui.Hide()
    }

    LoadControls() {
        config := this.app.config
        this.controls["hotkey_enabled"].Value := config.Bool("hotkey_enabled")
        this.controls["record_hotkey"].Text := config.Get("record_hotkey")
        this.controls["cancel_hotkey"].Text := config.Get("cancel_hotkey", "CapsLock+A")
        this.controls["toggle_delivery_hotkey"].Text := config.Get("toggle_delivery_hotkey", "CapsLock+D")
        this.ChooseText(this.controls["capslock_behavior"], config.Get("capslock_behavior", "preserve_tap"))
        this.controls["audio_gain"].Value := config.Get("audio_gain", "1")
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
        this.ChooseText(this.controls["inference_device"], config.Get("inference_device", "auto"))
        this.ChooseText(this.controls["ui_theme"], config.Get("ui_theme", "auto"))
        this.controls["parakeet_runtime_dir"].Value := config.Get("parakeet_runtime_dir_resolved", config.Get("parakeet_runtime_dir"))
        this.controls["model_dir"].Value := config.Get("model_dir_resolved", config.Get("model_dir"))
    }

    ChooseText(control, text) {
        try control.Choose(text)
        catch
            control.Choose(1)
        if control.Text != text
            control.Choose(1)
    }

    ResetDropDown(key, values, selectedText := "") {
        control := this.controls[key]
        control.Delete()
        control.Add(values)
        if selectedText != "" {
            try control.Choose(selectedText)
        }
        if control.Text = ""
            control.Choose(1)
    }

    FormatModelChoice(value) {
        parts := StrSplit(value, "|")
        filename := parts[1]
        sizeMb := parts.Length >= 2 ? parts[2] : "0"
        quant := parts.Length >= 4 ? parts[4] : "unknown"
        label := filename . " — " . quant
        if sizeMb != "0"
            label .= " — " . sizeMb . " MB"
        return Map("label", label, "filename", filename)
    }

    FindModelChoice(values, choices, filename) {
        if filename = ""
            return values.Length ? values[1] : ""
        for value in values {
            if choices.Has(value) && choices[value] = filename
                return value
        }
        return values.Length ? values[1] : ""
    }

    InferenceDeviceChanged(*) {
        device := this.controls["inference_device"].Text
        if device = "cpu"
            this.controls["device_hint"].Text := "CPU avoids VRAM use."
        else if device = "nvidia_gpu"
            this.controls["device_hint"].Text := "NVIDIA GPU is faster."
        else
            this.controls["device_hint"].Text := "Auto uses NVIDIA GPU when available, otherwise CPU."
        this.ListModels()
    }

    ThemeChanged(*) {
        if this.loadingControls
            return
        mode := this.controls["ui_theme"].Text
        if mode = ""
            return
        this.themeMode := mode
        this.app.config.Set("ui_theme", mode)
        SetTimer(ObjBindMethod(this, "ReopenAfterThemeChange"), -1)
    }

    ReopenAfterThemeChange(*) {
        mode := this.themeMode
        if IsObject(this.gui) {
            this.gui.Destroy()
            this.gui := ""
        }
        this.themeMode := mode
        this.Open(false)
        this.ChooseText(this.controls["ui_theme"], mode)
        this.SetStatus("Theme preview applied — press Save changes to keep it")
    }

    Save(*) {
        config := this.app.config
        config.Set("record_hotkey", this.controls["record_hotkey"].Text)
        config.Set("cancel_hotkey", this.controls["cancel_hotkey"].Text)
        config.Set("toggle_delivery_hotkey", this.controls["toggle_delivery_hotkey"].Text)
        for key in ["audio_gain", "typing_chunk_chars", "typing_interval_ms", "idle_worker_timeout_secs", "worker_shutdown_grace_ms", "parakeet_runtime_dir", "model_dir"]
            config.Set(key, this.controls[key].Value)
        for key in ["hotkey_enabled", "trailing_space", "remove_punctuation", "lowercase_output", "start_with_windows", "diagnostic_overlay", "log_transcripts"]
            config.Set(key, SimpleSttBoolText(this.controls[key].Value))
        selectedInput := this.controls["audio_device_contains"].Text
        config.Set("audio_device_contains", selectedInput = "Default microphone" ? "" : selectedInput)
        installedValue := this.controls["selected_model_filename"].Text
        if installedValue != "" && installedValue != "No installed models found" && this.installedModelChoices.Has(installedValue)
            config.Set("selected_model_filename", this.installedModelChoices[installedValue])
        config.Set("capslock_behavior", this.controls["capslock_behavior"].Text)
        config.Set("text_delivery_mode", this.controls["text_delivery_mode"].Text)
        config.Set("log_level", this.controls["log_level"].Text)
        config.Set("inference_device", this.controls["inference_device"].Text)
        config.Set("ui_theme", this.controls["ui_theme"].Text)
        try {
            HotkeySpec.Parse(config.Get("record_hotkey"))
            HotkeySpec.Parse(config.Get("cancel_hotkey"))
            HotkeySpec.Parse(config.Get("toggle_delivery_hotkey"))
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
            this.loadingControls := true
            this.LoadControls()
            this.loadingControls := false
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

    CaptureToggleHotkey(*) {
        if this.app.HasProp("testMode") && this.app.testMode {
            this.SetStatus("Preview: toggle shortcut recorder opened safely")
            return
        }
        this.SetStatus("Hold the desired modifiers, then press the final key for the delivery-mode toggle.")
        this.recorder.Start(ObjBindMethod(this, "ToggleHotkeyCaptured"))
    }

    CaptureCancelHotkey(*) {
        if this.app.HasProp("testMode") && this.app.testMode {
            this.SetStatus("Preview: cancel shortcut recorder opened safely")
            return
        }
        this.SetStatus("Hold the desired modifiers, then press the final key for global cancel.")
        this.recorder.Start(ObjBindMethod(this, "CancelHotkeyCaptured"))
    }

    HotkeyCaptured(label) {
        this.controls["record_hotkey"].Text := label
        this.SetStatus("Recorded " . label . ". Press Save to apply it.")
    }

    ToggleHotkeyCaptured(label) {
        this.controls["toggle_delivery_hotkey"].Text := label
        this.SetStatus("Recorded toggle hotkey " . label . ". Press Save to apply it.")
    }

    CancelHotkeyCaptured(label) {
        this.controls["cancel_hotkey"].Text := label
        this.SetStatus("Recorded cancel hotkey " . label . ". Press Save to apply it.")
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
        selected := this.app.config.Get("audio_device_contains")
        values := ["Default microphone"]
        for key, value in response["values"]
            if InStr(key, "input.") = 1
                values.Push(value)
        this.ResetDropDown("audio_device_contains", values, selected = "" ? "Default microphone" : selected)
        this.SetStatus("Microphone list refreshed")
    }

    ChooseInput(*) {
        value := this.controls["audio_device_contains"].Text
        if value = ""
            this.controls["audio_device_contains"].Choose(1)
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
        installed := Array()
        catalog := Array()
        this.installedModelChoices := Map()
        this.catalogModelChoices := Map()
        currentInstalled := this.app.config.Get("selected_model_filename")
        recommended := response["values"].Get("recommended_model", "")
        for key, value in response["values"] {
            if InStr(key, "installed_model.") = 1 {
                choice := this.FormatModelChoice(value)
                installed.Push(choice["label"])
                this.installedModelChoices[choice["label"]] := choice["filename"]
            } else if InStr(key, "catalog_model.") = 1 {
                choice := this.FormatModelChoice(value)
                catalog.Push(choice["label"])
                this.catalogModelChoices[choice["label"]] := choice["filename"]
            }
        }
        if !installed.Length
            installed.Push("No installed models found")
        if !catalog.Length
            catalog.Push("No downloadable models available")
        this.ResetDropDown("selected_model_filename", installed, this.FindModelChoice(installed, this.installedModelChoices, currentInstalled))
        this.ResetDropDown("model_list", catalog, this.FindModelChoice(catalog, this.catalogModelChoices, recommended))
        this.SetStatus("Installed and downloadable model lists refreshed")
    }

    ChooseInstalledModel(*) {
        value := this.controls["selected_model_filename"].Text
        if value = "No installed models found"
            return
        if !this.installedModelChoices.Has(value)
            return
        this.SetStatus("Selected installed model: " . this.installedModelChoices[value])
    }

    ChooseCatalogModel(*) {
        value := this.controls["model_list"].Text
        if value = "" || value = "No downloadable models available"
            return
        if !this.catalogModelChoices.Has(value)
            return
        filename := this.catalogModelChoices[value]
        this.SetStatus("Selected downloadable model: " . filename)
    }

    DownloadModel(*) {
        value := this.controls["model_list"].Text
        if value = "" || value = "No downloadable models available" {
            this.SetStatus("Choose a downloadable model first")
            return
        }
        if !this.catalogModelChoices.Has(value) {
            this.SetStatus("Choose a downloadable model first")
            return
        }
        filename := this.catalogModelChoices[value]
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
            case "model_download_complete":
                this.SetStatus("Model download complete: " . event["values"].Get("filename", ""))
                this.ListModels()
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
        this.CaptureCancelHotkey()
        this.CaptureToggleHotkey()
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
