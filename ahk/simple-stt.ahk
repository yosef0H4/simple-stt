#Requires AutoHotkey v2.0
#SingleInstance Force

#Include lib\Utils.ahk
#Include lib\TabProtocol.ahk
#Include lib\Logging.ahk
#Include lib\Config.ahk
#Include lib\IpcClient.ahk
#Include lib\ProcessSupervisor.ahk
#Include lib\Hotkeys.ahk
#Include lib\TextTransform.ahk
#Include lib\Typist.ahk
#Include lib\Tray.ahk

class SimpleSttShell {
    __New() {
        this.ctlExe := SimpleSttResolveExe("simple-stt-ctl")
        this.captureExe := SimpleSttResolveExe("simple-stt-capture")
        this.settingsExe := SimpleSttResolveExe("simple-stt-settings")
        if !FileExist(this.ctlExe)
            throw Error("Missing simple-stt-ctl.exe. Build or package the Rust binaries beside the shell.")
        if !FileExist(this.captureExe)
            throw Error("Missing simple-stt-capture.exe. Build or package the Rust binaries beside the shell.")
        if !FileExist(this.settingsExe)
            throw Error("Missing simple-stt-settings.exe. Build or package the Rust binaries beside the shell.")
        this.config := ConfigStore(this.ctlExe)
        this.logger := ShellLog(this.config.Get("shell_log_path"), this.EffectiveLogLevel())
        this.logger.Write("info", "shell start")
        this.sessionId := 0
        this.activeRecordingSession := 0
        this.sessions := Map()
        this.pendingStarts := Map()
        this.pendingStops := Map()
        this.supervisor := ProcessSupervisor(this.captureExe, this.ctlExe, this.config, this.logger, ObjBindMethod(this, "OnServiceRestart"))
        this.ipc := IpcClient(this.ctlExe, this.supervisor.stateFile, this.supervisor.token, ObjBindMethod(this, "HandleServiceEvent"), this.logger)
        this.supervisor.AttachIpc(this.ipc)
        this.typist := Typist(this.logger, ObjBindMethod(this, "Notice"))
        this.capsController := CapsLockTapController(this.logger)
        this.hotkeys := HotkeyManager(ObjBindMethod(this, "RecordDown"), ObjBindMethod(this, "RecordUp"), this.logger, this.capsController)
        this.cancelHotkey := HotkeyManager(ObjBindMethod(this, "CancelAll"), ObjBindMethod(this, "NoopHotkeyUp"), this.logger, this.capsController)
        this.deliveryToggleHotkey := HotkeyManager(ObjBindMethod(this, "ToggleDeliveryModeHotkey"), ObjBindMethod(this, "NoopHotkeyUp"), this.logger, this.capsController)
        this.cleanupToggleHotkey := HotkeyManager(ObjBindMethod(this, "ToggleCleanupHotkey"), ObjBindMethod(this, "NoopHotkeyUp"), this.logger, this.capsController)
        this.tray := TrayController(this)
        this.modeTooltipTimer := ObjBindMethod(this, "HideModeTooltip")
        this.ApplyHotkeyConfig()
        this.ApplyStartupRegistration()
        this.supervisor.Start()
    }

    ApplyHotkeyConfig() {
        try {
            capsMode := this.config.Get("capslock_behavior", "preserve_tap")
            releaseStops := this.config.Get("recording_mode", "hold") != "toggle"
            this.hotkeys.Configure(this.config.Get("record_hotkey", "CapsLock+S"), this.config.Bool("hotkey_enabled", true), capsMode, releaseStops)
            this.cancelHotkey.Configure(this.config.Get("cancel_hotkey", "CapsLock+A"), true, capsMode)
            this.deliveryToggleHotkey.Configure(this.config.Get("toggle_delivery_hotkey", "CapsLock+D"), true, capsMode)
            this.cleanupToggleHotkey.Configure(this.config.Get("toggle_cleanup_hotkey", "None"), true, capsMode)
        } catch Error as err {
            this.logger.Write("error", "hotkey configuration failed: " . err.Message)
            MsgBox(err.Message, "SimpleStt hotkey error", "Iconx")
        }
    }

    RecordDown() {
        if this.config.Get("recording_mode", "hold") = "toggle" && this.activeRecordingSession {
            this.RecordUp()
            return
        }
        if !this.ipc.ready {
            this.Notice("Audio service is not ready", "warning")
            return
        }
        target := WinActive("A")
        if !target {
            this.Notice("Recording cancelled: no active target window", "warning")
            return
        }
        this.sessionId += 1
        session := this.sessionId
        this.activeRecordingSession := session
        this.sessions[session] := target
        this.pendingStarts[session] := true
        this.logger.Write("info", "hotkey down target_hwnd=" . target, session)
        this.ipc.CallService("start-recording --session-id " . session . " --target-window " . target, ObjBindMethod(this, "RecordingStarted", session))
    }

    RecordingStarted(session, response) {
        if this.pendingStarts.Has(session)
            this.pendingStarts.Delete(session)
        if !response["ok"] {
            this.logger.Write("error", "recording start rejected: " . response["message"], session)
            this.Notice("Audio service rejected recording — see log", "error")
            this.ipc.CallService("stop-recording --session-id " . session)
            if this.activeRecordingSession = session
                this.activeRecordingSession := 0
            if this.sessions.Has(session)
                this.sessions.Delete(session)
            if this.pendingStops.Has(session)
                this.pendingStops.Delete(session)
            return
        }
        if this.pendingStops.Has(session) {
            this.pendingStops.Delete(session)
            this.SendStop(session)
        }
    }

    RecordUp() {
        if !this.activeRecordingSession
            return
        session := this.activeRecordingSession
        this.activeRecordingSession := 0
        this.logger.Write("info", "hotkey up", session)
        if this.pendingStarts.Has(session) {
            this.pendingStops[session] := true
            this.logger.Write("debug", "recording stop deferred until start acknowledgement", session)
            return
        }
        this.SendStop(session)
    }

    SendStop(session) {
        this.ipc.CallService("stop-recording --session-id " . session, ObjBindMethod(this, "RecordingStopped", session))
    }

    RecordingStopped(session, response) {
        if response["ok"]
            return
        this.logger.Write("error", "recording stop rejected: " . response["message"], session)
        this.Notice("Recording failed — see log", "error")
        if this.sessions.Has(session)
            this.sessions.Delete(session)
    }

    HandleServiceEvent(event) {
        kind := event["kind"]
        session := event["session_id"] = "" ? 0 : event["session_id"] + 0
        switch kind {
            case "service_ready": this.logger.Write("info", "service ready event")
            case "recording_started": this.logger.Write("info", "recording started event", session)
            case "transcribing": this.logger.Write("info", "transcribing event", session)
            case "transcript":
                if !this.sessions.Has(session) {
                    this.logger.Write("warning", "discarded transcript for unknown session", session)
                    return
                }
                target := this.sessions[session]
                this.sessions.Delete(session)
                text := this.TransformTranscript(event["text"])
                this.logger.Write("info", "transcript received chars=" . StrLen(text), session)
                mode := this.DeliveryModeForWindow(target)
                this.typist.Begin(session, target, text, this.config.Bool("paced_typing_enabled", true), this.config.Int("typing_speed_wpm", 450), this.config.Bool("trailing_space", true), mode)
            case "notice":
                this.Notice(event["text"], event["level"])
                if session && this.sessions.Has(session) && SimpleSttNoticeEndsSession(event)
                    this.sessions.Delete(session)
            case "configuration_reloaded":
                this.ApplyReloadedConfig()
            default:
                this.logger.Write("debug", "service event kind=" . kind, session)
        }
    }

    DeliveryModeForWindow(targetWindow) {
        configured := this.config.Get("text_delivery_mode", "smart_paste")
        try appId := WinGetProcessName("ahk_id " . targetWindow)
        catch
            return configured
        raw := this.config.Get("app_delivery_overrides", "[]")
        ; The settings service serializes each override as two JSON strings.
        ; Match the executable case-insensitively without introducing a JSON
        ; dependency into the always-running Windows shell.
        ; \Q...\E keeps valid executable characters such as +, (, and [ from
        ; becoming regular-expression operators.
        pattern := 'i)\{"app_id"\s*:\s*"\Q' . appId . '\E"\s*,\s*"mode"\s*:\s*"([a-z_]+)"\}'
        return RegExMatch(raw, pattern, &match) ? match[1] : configured
    }

    OnServiceRestart() {
        hadActive := this.activeRecordingSession || this.sessions.Count || this.typist.active || this.typist.queue.Length
        this.typist.Cancel("recording cancelled: audio service restarted", false)
        if this.activeRecordingSession
            this.logger.Write("warning", "recording cancelled: audio service restarted", this.activeRecordingSession)
        this.activeRecordingSession := 0
        this.sessions := Map()
        this.pendingStarts := Map()
        this.pendingStops := Map()
        this.Notice(hadActive ? "Recording cancelled: audio service restarted" : "Audio service restarting…", hadActive ? "warning" : "info")
    }

    TransformTranscript(text) {
        return SimpleSttTransformTranscript(text, this.config.Bool("remove_punctuation"), this.config.Bool("lowercase_output"))
    }

    Notice(text, level := "info") {
        this.logger.Write(level, text)
    }

    OpenSettings(*) {
        this.logger.Write("info", "settings requested executable=" . this.settingsExe)
        command := SimpleSttQuote(this.settingsExe) . " --state-file " . SimpleSttQuote(this.supervisor.stateFile) . " --service-token " . SimpleSttQuote(this.supervisor.token)
        try {
            Run(command, A_ScriptDir, "Hide", &settingsPid)
            this.logger.Write("info", "settings process launched pid=" . settingsPid)
        }
        catch Error as err {
            this.logger.Write("error", "settings launch failed: " . err.Message)
            MsgBox(err.Message, "SimpleStt settings error", "Iconx")
        }
    }

    NoopHotkeyUp() {
    }

    CancelAll(*) {
        hadActive := this.activeRecordingSession || this.sessions.Count || this.pendingStarts.Count || this.pendingStops.Count || this.typist.active || this.typist.queue.Length
        if this.typist.active || this.typist.queue.Length
            this.typist.Cancel("text delivery cancelled by global cancel", false)
        if this.activeRecordingSession
            this.logger.Write("warning", "recording cancelled by global cancel", this.activeRecordingSession)
        this.activeRecordingSession := 0
        this.sessions := Map()
        this.pendingStarts := Map()
        this.pendingStops := Map()
        if this.ipc.ready
            this.ipc.CallService("cancel")
        if hadActive
            this.Notice("Cancelled", "warning")
        else
            this.logger.Write("info", "global cancel pressed with no active shell work")
    }

    ToggleDeliveryModeHotkey() {
        current := this.config.Get("text_delivery_mode", "smart_paste")
        next := SimpleSttNextDeliveryMode(current, this.config.Get("enabled_delivery_modes", "smart_paste,type"))
        this.config.Set("text_delivery_mode", next)
        try {
            this.config.SaveSync()
            this.PublishRuntimeConfigChange()
            this.ShowDeliveryModeTooltip(next)
            this.logger.Write("info", "delivery mode toggled mode=" . next)
        } catch Error as err {
            this.logger.Write("error", "delivery mode toggle failed: " . err.Message)
            this.Notice("Delivery mode toggle failed — see log", "error")
        }
    }

    ShowDeliveryModeTooltip(mode) {
        labels := Map(
            "smart_paste", "Smart Paste",
            "type", "Typing",
            "clipboard", "Clipboard only",
            "paste_shift_insert", "Shift+Insert",
            "paste_ctrl_shift_v", "Ctrl+Shift+V",
            "paste_ctrl_v", "Ctrl+V"
        )
        message := "🎙 Delivery: " . (labels.Has(mode) ? labels[mode] : mode)
        ToolTip(message)
        SetTimer(this.modeTooltipTimer, -1200)
    }

    HideModeTooltip() {
        ToolTip()
    }

    ToggleHotkey(*) {
        enabled := !this.config.Bool("hotkey_enabled", true)
        this.config.Set("hotkey_enabled", SimpleSttBoolText(enabled))
        try {
            this.config.SaveSync()
            this.PublishRuntimeConfigChange()
            this.hotkeys.SetEnabled(enabled)
            this.tray.Rebuild()
            this.logger.Write("info", "hotkey enabled=" . SimpleSttBoolText(enabled))
            this.Notice(enabled ? "Hotkey enabled" : "Hotkey disabled")
        } catch Error as err {
            MsgBox(err.Message, "SimpleStt settings error", "Iconx")
        }
    }

    ToggleCleanupHotkey() {
        enabled := !this.config.Bool("cleanup_enabled", false)
        this.config.Set("cleanup_enabled", SimpleSttBoolText(enabled))
        try {
            this.config.SaveSync()
            this.PublishRuntimeConfigChange()
            ToolTip("AI cleanup: " . (enabled ? "On" : "Off"))
            SetTimer(this.modeTooltipTimer, -1200)
            this.logger.Write("info", "AI cleanup toggled enabled=" . SimpleSttBoolText(enabled))
        } catch Error as err {
            this.logger.Write("error", "AI cleanup toggle failed: " . err.Message)
            this.Notice("AI cleanup toggle failed — see log", "error")
        }
    }

    PublishRuntimeConfigChange() {
        if this.ipc.ready
            this.ipc.CallService("reload-config", ObjBindMethod(this, "ReloadServiceComplete"))
    }

    ReloadSettings(*) {
        try {
            this.config.LoadSync()
            this.logger.SetLevel(this.EffectiveLogLevel())
            this.ApplyHotkeyConfig()
            this.ApplyStartupRegistration()
            this.tray.Rebuild()
            this.ipc.CallService("reload-config", ObjBindMethod(this, "ReloadServiceComplete"))
            this.logger.Write("info", "settings reload requested")
        } catch Error as err {
            this.logger.Write("error", "settings reload failed: " . err.Message)
            MsgBox(err.Message, "SimpleStt settings error", "Iconx")
        }
    }

    ApplyReloadedConfig() {
        try {
            this.config.LoadSync()
            this.logger.SetLevel(this.EffectiveLogLevel())
            this.ApplyHotkeyConfig()
            this.ApplyStartupRegistration()
            this.tray.Rebuild()
            this.logger.Write("info", "settings applied from config reload")
        } catch Error as err {
            this.logger.Write("error", "browser settings apply failed: " . err.Message)
            this.Notice("Settings saved, but Windows hotkeys could not reload", "error")
        }
    }

    ApplySavedConfig() {
        this.logger.SetLevel(this.EffectiveLogLevel())
        this.ApplyHotkeyConfig()
        this.ApplyStartupRegistration()
        this.tray.Rebuild()
        this.ipc.CallService("reload-config", ObjBindMethod(this, "ReloadServiceComplete"))
        this.logger.Write("info", "settings changed")
    }

    EffectiveLogLevel() {
        return A_IsCompiled ? "minimal" : this.config.Get("log_level", "normal")
    }

    ReloadServiceComplete(response) {
        if !response["ok"] {
            this.Notice("Settings reload failed — see log", "error")
            this.logger.Write("error", "service config reload failed: " . response["message"])
            return
        }
        if response["values"].Has("restart_audio_service") && SimpleSttBool(response["values"]["restart_audio_service"])
            this.RestartAudioService()
    }

    RestartAudioService(*) {
        this.OnServiceRestart()
        this.supervisor.Restart()
    }

    ReloadApp(*) {
        this.logger.Write("info", "shell reload requested")
        Reload()
    }

    UnloadSpeechModel(*) {
        this.ipc.CallService("unload-model")
        this.logger.Write("info", "speech-model unload requested")
    }

    TestModel(*) {
        this.ipc.CallService("test-model")
        this.Notice("Model test queued")
    }

    OpenLatestLog(*) {
        path := this.config.Get("shell_log_path")
        if FileExist(path)
            Run(path)
        else {
            SplitPath(path, , &dir)
            Run(dir)
        }
    }

    ApplyStartupRegistration() {
        shortcut := A_Startup . "\SimpleStt.lnk"
        enabled := this.config.Bool("start_with_windows")
        if enabled {
            try {
                if A_IsCompiled
                    FileCreateShortcut(A_ScriptFullPath, shortcut, A_ScriptDir)
                else {
                    packagedLauncher := A_ScriptDir . "\..\simple-stt.cmd"
                    if FileExist(packagedLauncher)
                        FileCreateShortcut(packagedLauncher, shortcut, A_ScriptDir . "\..")
                    else
                        FileCreateShortcut(A_AhkPath, shortcut, A_ScriptDir, SimpleSttQuote(A_ScriptFullPath))
                }
            }
            catch Error as err
                this.logger.Write("warning", "startup shortcut create failed: " . err.Message)
        } else if FileExist(shortcut) {
            try FileDelete(shortcut)
        }
        this.logger.Write("info", "startup registration enabled=" . SimpleSttBoolText(enabled))
    }

    OnExit(reason, code) {
        this.logger.Write("info", "shell stop reason=" . reason . " code=" . code)
        this.typist.Cancel("shell exiting", false)
        this.hotkeys.DisableBindings()
        this.cancelHotkey.DisableBindings()
        this.deliveryToggleHotkey.DisableBindings()
        SetTimer(this.modeTooltipTimer, 0)
        ToolTip()
        this.ipc.Stop()
        this.supervisor.Shutdown()
    }

    OnError(error, mode) {
        try this.logger.Write("error", "unhandled shell error: " . error.Message . " mode=" . mode)
        return false
    }
}

try {
    global SimpleStt := SimpleSttShell()
    OnExit(ObjBindMethod(SimpleStt, "OnExit"))
    OnError(ObjBindMethod(SimpleStt, "OnError"))
    Persistent
} catch Error as err {
    MsgBox(err.Message, "SimpleStt startup error", "Iconx")
    ExitApp(1)
}
