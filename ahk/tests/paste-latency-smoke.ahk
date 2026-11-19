#Requires AutoHotkey v2.0
#SingleInstance Force

#Include ..\lib\Utils.ahk
#Include ..\lib\Logging.ahk
#Include ..\lib\Typist.ahk

if A_Args.Length && A_Args[1] = "--blocked-child" {
    target := Gui(, "SimpleStt blocked paste probe")
    targetEdit := target.AddEdit("w200", "blocked")
    subclass := CallbackCreate(SlowTextLength, , 6)
    DllCall("comctl32\SetWindowSubclass", "Ptr", targetEdit.Hwnd, "Ptr", subclass, "UPtr", 1, "Ptr", 0)
    target.Show("Hide")
    Persistent()
    return
}

SlowTextLength(hwnd, message, wParam, lParam, subclassId, referenceData) {
    if message = 0x000E {
        Sleep(2000)
        return 7
    }
    return DllCall("comctl32\DefSubclassProc", "Ptr", hwnd, "UInt", message, "UPtr", wParam, "Ptr", lParam, "Ptr")
}

NoopNotice(*) {
}

childPid := 0
exitCode := 0
childScript := A_Temp . "\simple-stt-paste-latency-child-" . A_TickCount . ".ahk"
try {
    childSource := StrReplace(FileRead(A_ScriptFullPath, "UTF-8"), "#Include ..\lib\", "#Include " . A_ScriptDir . "\..\lib\")
    FileAppend(childSource, childScript, "UTF-8")
    Run(SimpleSttQuote(A_AhkPath) . " /ErrorStdOut=UTF-8 " . SimpleSttQuote(childScript) . " --blocked-child", , "Hide", &childPid)
    DetectHiddenWindows(true)
    if !WinWait("SimpleStt blocked paste probe ahk_pid " . childPid, , 5)
        throw Error("blocked paste target did not start")
    controlHwnd := ControlGetHwnd("Edit1", "ahk_pid " . childPid)
    instance := Typist(ShellLog(A_Temp . "\simple-stt-paste-latency.log"), NoopNotice)
    started := A_TickCount
    result := instance.ReadEditText(controlHwnd)
    elapsed := A_TickCount - started
    if result["ok"]
        throw Error("blocked target unexpectedly answered")
    if elapsed > 300
        throw Error("paste probe stalled the shell for " . elapsed . " ms")
    SimpleSttConsoleLine("PASS: unresponsive paste target bounded to " . elapsed . " ms")
} catch Error as err {
    SimpleSttConsoleError("FAIL: " . err.Message)
    exitCode := 1
} finally {
    if childPid && ProcessExist(childPid)
        ProcessClose(childPid)
    if FileExist(childScript)
        FileDelete(childScript)
}
ExitApp(exitCode)
