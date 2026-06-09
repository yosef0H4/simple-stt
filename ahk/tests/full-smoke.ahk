#Requires AutoHotkey v2.0
#SingleInstance Force

#Include ..\lib\Utils.ahk
#Include ..\lib\TabProtocol.ahk
#Include ..\lib\Logging.ahk
#Include ..\lib\Config.ahk
#Include ..\lib\Typist.ahk

global SmokeCtl := ""
global SmokeCapture := ""
global SmokeConfig := ""
global SmokeState := ""
global SmokeToken := ""
global SmokePid := 0
global SmokeLatestSeq := 0
global SmokeTypingGui := ""

Info(message) {
    UvoxConsoleLine("INFO: " . message)
}

Fail(message, exitCode := 1) {
    UvoxConsoleError("FAIL: " . message)
    ExitApp(exitCode)
}

Assert(condition, message) {
    if !condition
        Fail(message)
}

GlobalErrorHandler(err, mode) {
    UvoxConsoleError("FAIL: unhandled AHK error: " . err.Message . " mode=" . mode)
    UvoxConsoleError("FILE: " . err.File . " LINE: " . err.Line)
    UvoxConsoleError("STACK: " . err.Stack)
    ExitApp(2)
    return true
}

NoopNotice(*) {
}

CallCtl(arguments) {
    global SmokeCtl, SmokeState, SmokeToken
    output := UvoxTempFile("full-smoke")
    command := UvoxQuote(SmokeCtl) . " --state-file " . UvoxQuote(SmokeState) . " --token " . UvoxQuote(SmokeToken) . " --output " . UvoxQuote(output) . " " . arguments
    try exitCode := RunWait(command, A_ScriptDir, "Hide")
    catch Error as err
        return TabProtocol.ErrorResponse("unable to run uvoxctl: " . err.Message)
    response := TabProtocol.ReadResponse(output)
    try FileDelete(output)
    if exitCode != 0 && response["ok"]
        return TabProtocol.ErrorResponse("uvoxctl exit code " . exitCode)
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
    SmokeToken := UvoxRandomToken()
    command := UvoxQuote(SmokeCapture) . " --token " . UvoxQuote(SmokeToken) . " --state-file " . UvoxQuote(SmokeState) . " --config " . UvoxQuote(SmokeConfig)
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
        UvoxConsoleError("WARN: graceful capture shutdown failed: " . response["message"])
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

TypingSmoke() {
    global SmokeTypingGui
    logger := ShellLog(A_Temp . "\uvox-full-smoke-typing.log")
    window := Gui("+AlwaysOnTop", "Uvox Full Smoke Typing")
    edit := window.AddEdit("w360 h80")
    window.Show("w390 h120")
    SmokeTypingGui := window
    WinWaitActive("ahk_id " . window.Hwnd, , 3)
    Assert(WinActive("A") = window.Hwnd, "typing test window did not become active")
    edit.Focus()
    typistInstance := Typist(logger, NoopNotice)
    typistInstance.Begin(9001, window.Hwnd, "hello world", 3, 10, false)
    deadline := A_TickCount + 5000
    while typistInstance.active && A_TickCount < deadline
        Sleep(25)
    Assert(!typistInstance.active, "typing smoke timed out")
    Assert(edit.Value = "hello world", "typing smoke mismatch: " . edit.Value)
    window.Destroy()
    SmokeTypingGui := ""
}

SetClipboardMarker() {
    format := DllCall("user32\RegisterClipboardFormatW", "Str", "UvoxSmokeObject", "UInt")
    if !DllCall("user32\OpenClipboard", "Ptr", 0, "Int")
        throw Error("unable to open clipboard for marker")
    try {
        DllCall("user32\EmptyClipboard")
        bytes := Buffer(12, 0)
        StrPut("uvox-marker", bytes, "UTF-8")
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

PasteSmoke() {
    global SmokeTypingGui
    logger := ShellLog(A_Temp . "\\uvox-full-smoke-paste.log")
    Hotkey("^+v", PastePlain, "On")
    for item in [["paste_ctrl_v", 9101], ["paste_ctrl_shift_v", 9102]] {
        format := SetClipboardMarker()
        window := Gui("+AlwaysOnTop", "Uvox Full Smoke Paste")
        edit := window.AddEdit("w360 h80")
        window.Show("w390 h120")
        SmokeTypingGui := window
        WinWaitActive("ahk_id " . window.Hwnd, , 3)
        Assert(WinActive("A") = window.Hwnd, item[1] . " window did not become active")
        edit.Focus()
        Sleep(100)
        typistInstance := Typist(logger, NoopNotice)
        typistInstance.Begin(item[2], window.Hwnd, "hello world", 3, 10, false, item[1])
        deadline := A_TickCount + 5000
        while typistInstance.active && A_TickCount < deadline
            Sleep(25)
        Assert(!typistInstance.active, item[1] . " smoke timed out")
        Assert(edit.Value = "hello world", item[1] . " smoke mismatch: " . edit.Value)
        Assert(DllCall("user32\IsClipboardFormatAvailable", "UInt", format, "Int"), item[1] . " did not restore non-text clipboard format")
        window.Destroy()
        SmokeTypingGui := ""
    }
    Hotkey("^+v", PastePlain, "Off")
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
    SmokeCtl := UvoxResolveExe("uvoxctl")
    SmokeCapture := UvoxResolveExe("uvox-capture")
    Assert(FileExist(SmokeCtl), "missing uvoxctl.exe")
    Assert(FileExist(SmokeCapture), "missing uvox-capture.exe")
    tempDir := A_Temp . "\uvox-full-smoke-" . A_TickCount
    DirCreate(tempDir)
    SmokeConfig := tempDir . "\config.json"
    SmokeState := tempDir . "\capture-state.json"
    EnvSet("UVOX_CONFIG", SmokeConfig)
    ConfigStore(SmokeCtl)

    Info("starting isolated capture service")
    StartCapture()
    response := CallCtl("list-inputs")
    Assert(response["ok"], "list-inputs failed: " . response["message"])
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

    Info("restarting isolated capture service")
    StopCapture()
    StartCapture()

    Info("typing hello world once")
    TypingSmoke()
    Info("pasting hello world once and restoring clipboard")
    PasteSmoke()
    StopCapture()
    UvoxConsoleLine("PASS: full AHK runtime smoke")
    ExitApp(0)
} catch Error as err {
    UvoxConsoleError("FAIL: " . err.Message)
    UvoxConsoleError("FILE: " . err.File . " LINE: " . err.Line)
    UvoxConsoleError("STACK: " . err.Stack)
    ExitApp(1)
}
