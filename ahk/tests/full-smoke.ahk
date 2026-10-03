#Requires AutoHotkey v2.0
#SingleInstance Force

#Include ..\lib\Utils.ahk
#Include ..\lib\TabProtocol.ahk
#Include ..\lib\Logging.ahk
#Include ..\lib\Config.ahk
#Include ..\lib\TextTransform.ahk
#Include ..\lib\Typist.ahk

global SmokeCtl := ""
global SmokeCapture := ""
global SmokeConfig := ""
global SmokeState := ""
global SmokeToken := ""
global SmokePid := 0
global SmokeLatestSeq := 0
global SmokeTypingGui := ""
global DelayedClipboardMarkers := ""

Info(message) {
    SimpleSttConsoleLine("INFO: " . message)
}

Fail(message, exitCode := 1) {
    SimpleSttConsoleError("FAIL: " . message)
    ExitApp(exitCode)
}

Assert(condition, message) {
    if !condition
        Fail(message)
}

GlobalErrorHandler(err, mode) {
    SimpleSttConsoleError("FAIL: unhandled AHK error: " . err.Message . " mode=" . mode)
    SimpleSttConsoleError("FILE: " . err.File . " LINE: " . err.Line)
    SimpleSttConsoleError("STACK: " . err.Stack)
    ExitApp(2)
    return true
}

NoopNotice(*) {
}

CallCtl(arguments) {
    global SmokeCtl, SmokeState, SmokeToken
    output := SimpleSttTempFile("full-smoke")
    command := SimpleSttQuote(SmokeCtl) . " --state-file " . SimpleSttQuote(SmokeState) . " --token " . SimpleSttQuote(SmokeToken) . " --output " . SimpleSttQuote(output) . " " . arguments
    try exitCode := RunWait(command, A_ScriptDir, "Hide")
    catch Error as err
        return TabProtocol.ErrorResponse("unable to run simple-stt-ctl: " . err.Message)
    response := TabProtocol.ReadResponse(output)
    try FileDelete(output)
    if exitCode != 0 && response["ok"]
        return TabProtocol.ErrorResponse("simple-stt-ctl exit code " . exitCode)
    return response
}

WaitForState(timeoutMs := 5000) {
    global SmokeState
    deadline := A_TickCount + timeoutMs
    while A_TickCount < deadline {
        if FileExist(SmokeState)
            return true
        Sleep(100)
    }
    return false
}

StartCapture() {
    global SmokeCapture, SmokeConfig, SmokeState, SmokeToken, SmokePid, SmokeLatestSeq
    try FileDelete(SmokeState)
    SmokeToken := SimpleSttRandomToken()
    command := SimpleSttQuote(SmokeCapture) . " --token " . SimpleSttQuote(SmokeToken) . " --state-file " . SimpleSttQuote(SmokeState) . " --config " . SimpleSttQuote(SmokeConfig)
    Run(command, A_ScriptDir, "Hide", &pid)
    SmokePid := pid
    SmokeLatestSeq := 0
    Assert(WaitForState(), "capture state file was not published")
    response := CallCtl("ping")
    Assert(response["ok"] && response["message"] = "pong", "capture ping failed: " . response["message"])
    return response
}

StopCapture() {
    global SmokePid
    if !SmokePid
        return
    response := CallCtl("shutdown")
    if !response["ok"]
        SimpleSttConsoleError("WARN: graceful capture shutdown failed: " . response["message"])
    try ProcessWaitClose(SmokePid, 3)
    if ProcessExist(SmokePid)
        try ProcessClose(SmokePid)
    SmokePid := 0
}

PollEvents() {
    global SmokeLatestSeq
    response := CallCtl("poll-events --after-seq " . SmokeLatestSeq)
    Assert(response["ok"], "poll-events failed: " . response["message"])
    if response["values"].Has("latest_seq")
        SmokeLatestSeq := response["values"]["latest_seq"] + 0
    return response["events"]
}

WaitForEvent(kind, timeoutMs := 120000) {
    deadline := A_TickCount + timeoutMs
    while A_TickCount < deadline {
        for event in PollEvents() {
            if event["kind"] = kind
                return event
            if event["kind"] = "notice" && event["level"] = "error"
                Fail("service notice: " . event["text"])
        }
        Sleep(150)
    }
    Fail("timed out waiting for event: " . kind)
}

WaitForWorkerLoaded(timeoutMs := 10000) {
    deadline := A_TickCount + timeoutMs
    while A_TickCount < deadline {
        response := CallCtl("ping")
        Assert(response["ok"], "ping during warm-up failed: " . response["message"])
        if response["values"].Has("worker_pid")
            return true
        Sleep(150)
    }
    return false
}

WaitForWorkerUnloaded(timeoutMs := 10000) {
    deadline := A_TickCount + timeoutMs
    while A_TickCount < deadline {
        response := CallCtl("ping")
        Assert(response["ok"], "ping during unload failed: " . response["message"])
        if !response["values"].Has("worker_pid")
            return true
        Sleep(150)
    }
    return false
}

KeyboardLanguageSmoke() {
    global SmokeCtl, SmokeTypingGui
    settings := ConfigStore(SmokeCtl)
    settings.Set("selection_mode", "follow_keyboard")
    settings.SaveSync()
    response := CallCtl("reload-config")
    Assert(response["ok"], "language-mode reload failed")
    window := Gui("+AlwaysOnTop", "SimpleStt Language Smoke")
    edit := window.AddEdit("w360 h80")
    window.Show("w390 h120")
    SmokeTypingGui := window
    WinActivate("ahk_id " . window.Hwnd)
    threadId := DllCall("GetWindowThreadProcessId", "Ptr", window.Hwnd, "Ptr", 0, "UInt")
    originalLayout := DllCall("GetKeyboardLayout", "UInt", threadId, "Ptr")
    try {
        for language in [Map("klid", "00000409", "name", "en", "session", 7201), Map("klid", "00000401", "name", "ar", "session", 7202)] {
            layout := DllCall("LoadKeyboardLayoutW", "Str", language["klid"], "UInt", 1, "Ptr")
            Assert(layout != 0, "test keyboard layout unavailable: " . language["name"])
            DllCall("ActivateKeyboardLayout", "Ptr", layout, "UInt", 0, "Ptr")
            Sleep(150)
            activeLayout := DllCall("GetKeyboardLayout", "UInt", threadId, "Ptr")
            Assert((activeLayout & 0x3ff) = (layout & 0x3ff), "test window did not switch keyboard layout")
            response := CallCtl("start-recording --session-id " . language["session"] . " --target-window " . window.Hwnd)
            Assert(response["ok"], "language recording start failed: " . response["message"])
            event := WaitForEvent("recording_started")
            Assert(event["values"]["language"] = language["name"], "recording selected the wrong speech language")
            response := CallCtl("cancel")
            Assert(response["ok"], "language test cancellation failed")
        }
        WinActivate("ahk_id " . window.Hwnd)
        WinWaitActive("ahk_id " . window.Hwnd, , 2)
        edit.Focus()
        Sleep(100)
        SendText("مرحبا")
        Sleep(100)
        Assert(InStr(edit.Value, "مرحبا"), "Arabic text did not stay in the controlled edit box: " . edit.Value)
    } finally {
        DllCall("ActivateKeyboardLayout", "Ptr", originalLayout, "UInt", 0, "Ptr")
        window.Destroy()
        SmokeTypingGui := ""
        settings.Set("selection_mode", "single_model")
        settings.SaveSync()
        CallCtl("reload-config")
    }
}

TypingSmoke() {
    global SmokeTypingGui
    logger := ShellLog(A_Temp . "\simple-stt-full-smoke-typing.log")
    window := Gui("+AlwaysOnTop", "SimpleStt Full Smoke Typing")
    edit := window.AddEdit("w360 h80")
    window.Show("w390 h120")
    SmokeTypingGui := window
    WinWaitActive("ahk_id " . window.Hwnd, , 3)
    Assert(WinActive("A") = window.Hwnd, "typing test window did not become active")
    edit.Focus()
    typistInstance := Typist(logger, NoopNotice)
    typistInstance.Begin(9001, window.Hwnd, "hello world", false, 100, false)
    deadline := A_TickCount + 5000
    while typistInstance.active && A_TickCount < deadline
        Sleep(25)
    Assert(!typistInstance.active, "typing smoke timed out")
    Assert(edit.Value = "hello world", "typing smoke mismatch: " . edit.Value)
    window.Destroy()
    SmokeTypingGui := ""
}

SetClipboardMarker() {
    format := DllCall("user32\RegisterClipboardFormatW", "Str", "SimpleSttSmokeObject", "UInt")
    if !DllCall("user32\OpenClipboard", "Ptr", 0, "Int")
        throw Error("unable to open clipboard for marker")
    try {
        DllCall("user32\EmptyClipboard")
        marker := "simple-stt-marker"
        bytes := Buffer(StrPut(marker, "UTF-8"), 0)
        StrPut(marker, bytes, "UTF-8")
        handle := DllCall("kernel32\GlobalAlloc", "UInt", 0x42, "UPtr", bytes.Size, "Ptr")
        if !handle
            throw Error("unable to allocate clipboard marker")
        ptr := DllCall("kernel32\GlobalLock", "Ptr", handle, "Ptr")
        DllCall("ntdll\RtlMoveMemory", "Ptr", ptr, "Ptr", bytes.Ptr, "UPtr", bytes.Size)
        DllCall("kernel32\GlobalUnlock", "Ptr", handle)
        if !DllCall("user32\SetClipboardData", "UInt", format, "Ptr", handle, "Ptr")
            throw Error("unable to publish clipboard marker")
    } finally DllCall("user32\CloseClipboard")
    return format
}

PastePlain(*) {
    Send("^v")
}

DelayedEditPasteProc(hwnd, message, wParam, lParam, subclassId, refData) {
    global DelayedPasteHwnd, DelayedPasteText, DelayedPasteReadAtTimer, DelayedClipboardMarkers
    if message = 0x0302 { ; WM_PASTE
        DelayedPasteHwnd := hwnd
        if !DelayedPasteReadAtTimer
            DelayedPasteText := A_Clipboard
        DelayedClipboardMarkers := ReadClipboardHistoryMarkers()
        SetTimer(InsertDelayedPaste, -650)
        return 0
    }
    return DllCall("comctl32\DefSubclassProc", "Ptr", hwnd, "UInt", message, "UPtr", wParam, "Ptr", lParam, "Ptr")
}

InsertDelayedPaste() {
    global DelayedPasteHwnd, DelayedPasteText, DelayedPasteReadAtTimer
    if !DelayedPasteHwnd
        return
    if DelayedPasteReadAtTimer
        DelayedPasteText := A_Clipboard
    textPointer := StrPtr(DelayedPasteText)
    DllCall("user32\SendMessageW", "Ptr", DelayedPasteHwnd, "UInt", 0x00C2, "UPtr", 1, "Ptr", textPointer, "Ptr") ; EM_REPLACESEL
    DelayedPasteHwnd := 0
}

ReadClipboardHistoryMarkers() {
    names := ["ExcludeClipboardContentFromMonitorProcessing", "CanIncludeInClipboardHistory", "CanUploadToCloudClipboard"]
    values := Array()
    if !DllCall("user32\OpenClipboard", "Ptr", A_ScriptHwnd, "Int")
        return "busy"
    try {
        for name in names {
            format := DllCall("user32\RegisterClipboardFormatW", "Str", name, "UInt")
            handle := DllCall("user32\GetClipboardData", "UInt", format, "Ptr")
            if !handle {
                values.Push("missing")
                continue
            }
            pointer := DllCall("kernel32\GlobalLock", "Ptr", handle, "Ptr")
            values.Push(pointer ? NumGet(pointer, 0, "UInt") : "unreadable")
            if pointer
                DllCall("kernel32\GlobalUnlock", "Ptr", handle)
        }
    } finally DllCall("user32\CloseClipboard")
    return values
}

DelayedPasteSmoke(logger, concurrentCopy, cleanClipboard := true, payload := "hello world  ") {
    global SmokeTypingGui, DelayedPasteCallback, DelayedPasteHwnd, DelayedPasteText, DelayedPasteReadAtTimer, DelayedClipboardMarkers
    format := SetClipboardMarker()
    window := Gui("+AlwaysOnTop", "SimpleStt Delayed Paste Smoke")
    edit := window.AddEdit("w360 h80")
    window.Show("w390 h120")
    SmokeTypingGui := window
    WinWaitActive("ahk_id " . window.Hwnd, , 3)
    Assert(WinActive("A") = window.Hwnd, "delayed paste window did not become active")
    edit.Focus()
    Sleep(100)
    DelayedPasteReadAtTimer := !concurrentCopy
    DelayedPasteHwnd := 0
    DelayedPasteText := ""
    DelayedClipboardMarkers := ""
    DelayedPasteCallback := CallbackCreate(DelayedEditPasteProc, "Fast", 6)
    Assert(DllCall("comctl32\SetWindowSubclass", "Ptr", edit.Hwnd, "Ptr", DelayedPasteCallback, "UPtr", 1, "Ptr", 0, "Int"), "failed to enable delayed paste test")
    typistInstance := Typist(logger, NoopNotice)
    startedAt := A_TickCount
    typistInstance.Begin(concurrentCopy ? 9106 : (cleanClipboard ? 9105 : 9107), window.Hwnd, payload, false, 100, false, "paste_ctrl_v", cleanClipboard)
    Sleep(150)
    if concurrentCopy
        A_Clipboard := "copied during paste"
    deadline := A_TickCount + 5000
    while typistInstance.active && A_TickCount < deadline
        Sleep(25)
    Assert(!typistInstance.active, "delayed paste smoke timed out")
    Assert(edit.Value = payload, "delayed paste smoke mismatch: " . edit.Value)
    Assert(A_TickCount - startedAt >= 600, "delayed paste completed before the edit processed WM_PASTE")
    if concurrentCopy
        Assert(A_Clipboard = "copied during paste", "paste cleanup overwrote a newer clipboard copy")
    else if cleanClipboard {
        Assert(IsObject(DelayedClipboardMarkers) && DelayedClipboardMarkers.Length = 3, "clean clipboard paste omitted history marker formats")
        for marker in DelayedClipboardMarkers
            Assert(marker = 0, "clean clipboard history marker was not DWORD zero")
        Assert(DllCall("user32\IsClipboardFormatAvailable", "UInt", format, "Int"), "verified delayed paste did not restore the non-text clipboard format")
    } else {
        Assert(IsObject(DelayedClipboardMarkers) && DelayedClipboardMarkers.Length = 3 && DelayedClipboardMarkers[1] = "missing" && DelayedClipboardMarkers[2] = "missing" && DelayedClipboardMarkers[3] = "missing", "clipboard option off still published history markers")
        Assert(A_Clipboard = payload, "clipboard option off did not retain the transcript")
    }
    DllCall("comctl32\RemoveWindowSubclass", "Ptr", edit.Hwnd, "Ptr", DelayedPasteCallback, "UPtr", 1)
    CallbackFree(DelayedPasteCallback)
    DelayedPasteCallback := 0
    window.Destroy()
    SmokeTypingGui := ""
}

PasteSmoke() {
    global SmokeTypingGui
    logger := ShellLog(A_Temp . "\\simple-stt-full-smoke-paste.log")
    Hotkey("^+v", PastePlain, "On")
    for item in [["smart_paste", 9100], ["paste_ctrl_v", 9101], ["paste_ctrl_shift_v", 9102], ["paste_shift_insert", 9103]] {
        format := SetClipboardMarker()
        window := Gui("+AlwaysOnTop", "SimpleStt Full Smoke Paste")
        edit := window.AddEdit("w360 h80")
        window.Show("w390 h120")
        SmokeTypingGui := window
        WinWaitActive("ahk_id " . window.Hwnd, , 3)
        Assert(WinActive("A") = window.Hwnd, item[1] . " window did not become active")
        edit.Focus()
        Sleep(100)
        typistInstance := Typist(logger, NoopNotice)
        startedAt := A_TickCount
        typistInstance.Begin(item[2], window.Hwnd, "hello world", false, 100, false, item[1])
        deadline := A_TickCount + 5000
        while typistInstance.active && A_TickCount < deadline
            Sleep(25)
        Assert(!typistInstance.active, item[1] . " smoke timed out")
        Assert(edit.Value = "hello world", item[1] . " smoke mismatch: " . edit.Value)
        Assert(DllCall("user32\IsClipboardFormatAvailable", "UInt", format, "Int"), item[1] . " did not restore non-text clipboard format")
        window.Destroy()
        SmokeTypingGui := ""
    }
    DelayedPasteSmoke(logger, false)
    DelayedPasteSmoke(logger, false, false)
    DelayedPasteSmoke(logger, false, true, "界")
    DelayedPasteSmoke(logger, true)
    window := Gui("+AlwaysOnTop", "SimpleStt Unknown Paste Target")
    button := window.AddButton("w180 h40", "No Edit Control")
    window.Show("w220 h80")
    SmokeTypingGui := window
    WinWaitActive("ahk_id " . window.Hwnd, , 3)
    button.Focus()
    A_Clipboard := "prior clipboard"
    typistInstance := Typist(logger, NoopNotice)
    typistInstance.Begin(9104, window.Hwnd, "retain transcript", false, 100, false, "paste_ctrl_v")
    deadline := A_TickCount + 5000
    while typistInstance.active && A_TickCount < deadline
        Sleep(25)
    Assert(!typistInstance.active, "unknown-control paste did not complete its delivery lifecycle")
    Assert(A_Clipboard = "retain transcript", "unknown-control paste did not retain the transcript clipboard")
    window.Destroy()
    SmokeTypingGui := ""
    Hotkey("^+v", PastePlain, "Off")
}

DeliveryCachePayloadSmoke() {
    Assert(SimpleSttFinalDeliveryPayload("你好🙂", true) = "你好🙂 ", "delivery cache payload did not retain one configured trailing space")
    Assert(SimpleSttFinalDeliveryPayload("你好🙂 ", false) = "你好🙂 ", "retry payload gained or lost trailing whitespace")
    Assert(SimpleSttFinalDeliveryPayload("", true) = "", "empty transcript produced a retry payload")
}

Cleanup(*) {
    global SmokePid, SmokeTypingGui
    if IsObject(SmokeTypingGui)
        try SmokeTypingGui.Destroy()
    if SmokePid
        try StopCapture()
}

OnError(GlobalErrorHandler)
OnExit(Cleanup)

try {
    SmokeCtl := SimpleSttResolveExe("simple-stt-ctl")
    SmokeCapture := SimpleSttResolveExe("simple-stt-capture")
    Assert(FileExist(SmokeCtl), "missing simple-stt-ctl.exe")
    Assert(FileExist(SmokeCapture), "missing simple-stt-capture.exe")
    tempDir := A_Temp . "\simple-stt-full-smoke-" . A_TickCount
    DirCreate(tempDir)
    SmokeConfig := tempDir . "\config.json"
    SmokeState := tempDir . "\capture-state.json"
    EnvSet("SIMPLE_STT_CONFIG", SmokeConfig)
    settings := ConfigStore(SmokeCtl)
    Info("starting isolated capture service")
    StartCapture()
    Info("checking inactive None selection")
    response := CallCtl("start-recording --session-id 7000")
    Assert(response["ok"] && response["values"].Has("recording") && response["values"]["recording"] = "skipped", "None did not skip recording")
    Sleep(200)
    for event in PollEvents()
        Assert(event["kind"] != "recording_started" && event["kind"] != "transcript" && event["kind"] != "model_loading", "None caused recording or inference")
    Assert(!CallCtl("ping")["values"].Has("worker_pid"), "None launched an inference worker")
    settings.Set("single_model_filename", "tdt_ctc-110m-q8_0.gguf")
    settings.Set("language_models", '{"en":"tdt_ctc-110m-q8_0.gguf","ar":"lemura-arabic-asr-lite-q8_0.gguf"}')
    settings.SaveSync()
    Assert(CallCtl("reload-config")["ok"], "model selection reload failed")
    response := CallCtl("list-inputs")
    Assert(response["ok"], "list-inputs failed: " . response["message"])
    inputLabels := 0
    for key, value in response["values"] {
        if RegExMatch(key, "^input\.(\d+)\.label$", &match) {
            inputLabels += 1
            Assert(value != "", "microphone label was empty")
            Assert(response["values"].Has("input." . match[1] . ".id"), "microphone stable ID was missing")
        }
    }
    Assert(inputLabels > 0, "list-inputs returned no microphones")
    response := CallCtl("list-models")
    Assert(response["ok"], "list-models failed: " . response["message"])

    Info("loading and testing real speech model")
    response := CallCtl("test-model")
    Assert(response["ok"], "test-model queue failed: " . response["message"])
    WaitForEvent("model_test_complete")
    response := CallCtl("ping")
    Assert(response["ok"] && response["values"].Has("worker_pid"), "worker PID not visible after model test")

    Info("unloading speech model worker")
    response := CallCtl("unload-model")
    Assert(response["ok"], "unload-model failed: " . response["message"])
    Assert(WaitForWorkerUnloaded(), "worker did not exit after unload")

    Info("warming speech model at recording start")
    response := CallCtl("start-recording --session-id 7001")
    Assert(response["ok"], "start-recording warm-up failed: " . response["message"])
    Assert(WaitForWorkerLoaded(), "worker PID not visible while recording was still active")

    Info("testing foreground keyboard language in a controlled edit window")
    KeyboardLanguageSmoke()

    Info("testing the Arabic model with real inference")
    response := CallCtl("test-model --language arabic")
    Assert(response["ok"], "Arabic model test queue failed: " . response["message"])
    WaitForEvent("model_test_complete")

    Info("restarting isolated capture service")
    StopCapture()
    StartCapture()

    Info("typing hello world once")
    TypingSmoke()
    DeliveryCachePayloadSmoke()
    Info("pasting hello world once and restoring clipboard")
    PasteSmoke()
    StopCapture()
    SimpleSttConsoleLine("PASS: full AHK runtime smoke")
    ExitApp(0)
} catch Error as err {
    SimpleSttConsoleError("FAIL: " . err.Message)
    SimpleSttConsoleError("FILE: " . err.File . " LINE: " . err.Line)
    SimpleSttConsoleError("STACK: " . err.Stack)
    ExitApp(1)
}
