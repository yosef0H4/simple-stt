#Requires AutoHotkey v2.0
#SingleInstance Force
#Include ..\lib\Utils.ahk

eventsFile := SimpleSttTempFile("recorded-action")
cancelFile := SimpleSttTempFile("recorded-action-cancel")
window := Gui("+AlwaysOnTop", "Recorded action smoke")
targetEdit := window.AddEdit("w320")
trace := []
interruptMode := ""
competitor := Gui("+AlwaysOnTop", "Recorded action competitor")
competitorEdit := competitor.AddEdit("w320")
competitorInput := 0
OnMessage(0x0100, ObserveKey)
OnMessage(0x0101, ObserveKey)
try {
    FileAppend("0`t57`t1`n30`t57`t0`n150`t57`t1`n180`t57`t0`n", eventsFile, "UTF-8-RAW")
    window.Show()
    WinActivate("ahk_id " window.Hwnd)
    if !WinWaitActive("ahk_id " window.Hwnd, , 3)
        throw Error("Test target did not become active")
    targetEdit.Focus()
    command := SimpleSttQuote(A_AhkPath) " /ErrorStdOut=UTF-8 " SimpleSttQuote(A_ScriptDir "\..\action-replay.ahk") " " window.Hwnd " " DllCall("GetCurrentProcessId", "UInt") " " SimpleSttQuote(eventsFile) " " SimpleSttQuote(cancelFile)
    if RunWait(command) != 0 || trace.Length != 4
        throw Error("Expected two paired key presses")
    if trace[3].at - trace[1].at < 80
        throw Error("Recorded timing was discarded")
    FileAppend("cancel", cancelFile)
    if RunWait(command) = 0 || trace.Length != 4
        throw Error("Canceled action delivered input")
    FileDelete(cancelFile)
    FileDelete(eventsFile)
    FileAppend("0`t29`t1`n100`t30`t1`n130`t30`t0`n160`t29`t0`n", eventsFile, "UTF-8-RAW")
    if RunWait(command) != 0 || GetKeyState("Control")
        throw Error("Modifier chord failed or Control remained held")
    for mode in ["cancel", "focus"] {
        WinActivate("ahk_id " window.Hwnd)
        if !WinWaitActive("ahk_id " window.Hwnd, , 3)
            throw Error("Target did not regain focus")
        targetEdit.Focus()
        FileDelete(eventsFile)
        FileAppend("0`t29`t1`n600`t30`t1`n630`t30`t0`n660`t29`t0`n", eventsFile, "UTF-8-RAW")
        interruptMode := mode
        if RunWait(command) = 0
            throw Error("Interrupted action succeeded")
        interruptMode := ""
        if GetKeyState("Control") || FileExist(eventsFile ".held")
            throw Error("Interrupted action left a held key")
        if competitorInput
            throw Error("Replay sent input to competing window")
        try FileDelete(cancelFile)
    }
    SimpleSttConsoleLine("PASS: recorded action timing, modifiers, cancellation, focus safety and held-key release")
} catch as caughtError {
    SimpleSttConsoleError("FAIL: " caughtError.Message)
    ExitApp(1)
} finally {
    competitor.Destroy()
    window.Destroy()
    try FileDelete(eventsFile)
    try FileDelete(cancelFile)
}
ExitApp(0)

ObserveKey(vk, details, message, hwnd) {
    global trace, targetEdit, interruptMode, cancelFile, competitor, competitorEdit, competitorInput
    if hwnd = competitorEdit.Hwnd && message = 0x0100
        competitorInput += 1
    if vk = 17 && message = 0x0100 && hwnd = targetEdit.Hwnd && interruptMode != "" {
        if interruptMode = "cancel"
            FileAppend("cancel", cancelFile)
        else {
            competitor.Show()
            WinActivate("ahk_id " competitor.Hwnd)
            competitorEdit.Focus()
        }
    }
    if vk = 32 && hwnd = targetEdit.Hwnd
        trace.Push({at:A_TickCount, message:message})
}
