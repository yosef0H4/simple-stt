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
#Include lib\SettingsGui.ahk

class UvoxShell {
    __New() {
        this.ctlExe := UvoxResolveExe("uvoxctl")
        this.captureExe := UvoxResolveExe("uvox-capture")
        if !FileExist(this.ctlExe)
            throw Error("Missing uvoxctl.exe. Build or package the Rust binaries beside the shell.")
        if !FileExist(this.captureExe)
            throw Error("Missing uvox-capture.exe. Build or package the Rust binaries beside the shell.")
        this.config := ConfigStore(this.ctlExe)
        this.logger := ShellLog(this.config.Get("shell_log_path"), this.config.Get("log_level", "normal"))
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
        this.hotkeys := HotkeyManager(ObjBindMethod(this, "RecordDown"), ObjBindMethod(this, "RecordUp"), this.logger)
        this.settings := SettingsGui(this)
        this.tray := TrayController(this)
        this.ApplyHotkeyConfig()
        this.ApplyStartupRegistration()
        this.supervisor.Start()
    }

    ApplyHotkeyConfig() {
        try this.hotkeys.Configure(this.config.Get("record_hotkey", "CapsLock+S"), this.config.Bool("hotkey_enabled", true), this.config.Get("capslock_behavior", "preserve_tap"))
        catch Error as err {
            this.logger.Write("error", "hotkey configuration failed: " . err.Message)
            MsgBox(err.Message, "Uvox hotkey error", "Iconx")
        }
    }

    RecordDown() {
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
        this.ipc.CallService("start-recording --session-id " . session, ObjBindMethod(this, "RecordingStarted", session))
    }

    RecordingStarted(session, response) {
        if this.pendingStarts.Has(session)
            this.pendingStarts.Delete(session)
        if !response["ok"] {
            this.logger.Write("error", "recording start rejected: " . response["message"], session)
            this.Notice("Audio service rejected recording — see log", "error")
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
                this.typist.Begin(session, target, text, this.config.Int("typing_chunk_chars", 3), this.config.Int("typing_interval_ms", 20), this.config.Bool("trailing_space", true), this.config.Get("text_delivery_mode", "paste_ctrl_v"))
            case "notice":
                this.Notice(event["text"], event["level"])
                if session && this.sessions.Has(session)
                    this.sessions.Delete(session)
            default:
                this.logger.Write("debug", "service event kind=" . kind, session)
        }
        this.settings.HandleEvent(event)
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
        return UvoxTransformTranscript(text, this.config.Bool("remove_punctuation"), this.config.Bool("lowercase_output"))
    }

    Notice(text, level := "info") {
        option := level = "error" ? 3 : level = "warning" ? 2 : 1
        this.logger.Write(level, text)
        TrayTip(text, "Uvox", option)
    }

    OpenSettings(*) {
        this.settings.Open()
    }

    ToggleHotkey(*) {
        enabled := !this.config.Bool("hotkey_enabled", true)
        this.config.Set("hotkey_enabled", UvoxBoolText(enabled))
        try {
            this.config.SaveSync()
            this.hotkeys.SetEnabled(enabled)
            this.tray.Rebuild()
            this.logger.Write("info", "hotkey enabled=" . UvoxBoolText(enabled))
            this.Notice(enabled ? "Hotkey enabled" : "Hotkey disabled")
        } catch Error as err {
            MsgBox(err.Message, "Uvox settings error", "Iconx")
        }
    }

    ReloadSettings(*) {
        try {
            this.config.LoadSync()
            this.logger.SetLevel(this.config.Get("log_level", "normal"))
            this.ApplyHotkeyConfig()
            this.ApplyStartupRegistration()
            this.tray.Rebuild()
            this.ipc.CallService("reload-config", ObjBindMethod(this, "ReloadServiceComplete"))
            this.logger.Write("info", "settings reload requested")
        } catch Error as err {
            this.logger.Write("error", "settings reload failed: " . err.Message)
            MsgBox(err.Message, "Uvox settings error", "Iconx")
        }
    }

    ApplySavedConfig() {
        this.logger.SetLevel(this.config.Get("log_level", "normal"))
        this.ApplyHotkeyConfig()
        this.ApplyStartupRegistration()
        this.tray.Rebuild()
        this.ipc.CallService("reload-config", ObjBindMethod(this, "ReloadServiceComplete"))
        this.logger.Write("info", "settings changed")
    }

    ReloadServiceComplete(response) {
        if !response["ok"] {
            this.Notice("Settings reload failed — see log", "error")
            this.logger.Write("error", "service config reload failed: " . response["message"])
            return
        }
        if response["values"].Has("restart_audio_service") && UvoxBool(response["values"]["restart_audio_service"])
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
        shortcut := A_Startup . "\Uvox.lnk"
        enabled := this.config.Bool("start_with_windows")
        if enabled {
            try FileCreateShortcut(A_ScriptFullPath, shortcut, A_ScriptDir)
            catch Error as err
                this.logger.Write("warning", "startup shortcut create failed: " . err.Message)
        } else if FileExist(shortcut) {
            try FileDelete(shortcut)
        }
        this.logger.Write("info", "startup registration enabled=" . UvoxBoolText(enabled))
    }

    OnExit(reason, code) {
        this.logger.Write("info", "shell stop reason=" . reason . " code=" . code)
        this.typist.Cancel("shell exiting", false)
        this.hotkeys.DisableBindings()
        this.ipc.Stop()
        this.supervisor.Shutdown()
    }

    OnError(error, mode) {
        try this.logger.Write("error", "unhandled shell error: " . error.Message . " mode=" . mode)
        return false
    }
}

try {
    global Uvox := UvoxShell()
    OnExit(ObjBindMethod(Uvox, "OnExit"))
    OnError(ObjBindMethod(Uvox, "OnError"))
    Persistent
} catch Error as err {
    MsgBox(err.Message, "Uvox startup error", "Iconx")
    ExitApp(1)
}
