#Requires AutoHotkey v2.0
#SingleInstance Force
#Include ..\lib\Utils.ahk

eventsFile := SimpleSttTempFile("recorded-action")
cancelFile := SimpleSttTempFile("recorded-action-cancel")
window := Gui("+AlwaysOnTop", "Recorded action smoke")
targetEdit := window.AddEdit("w320")
trace := []
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
    SimpleSttConsoleLine("PASS: recorded action pairing, timing and cancellation")
} catch as error {
    SimpleSttConsoleError("FAIL: " error.Message)
    ExitApp(1)
} finally {
    window.Destroy()
    try FileDelete(eventsFile)
    try FileDelete(cancelFile)
}
ExitApp(0)

ObserveKey(vk, details, message, hwnd) {
    global trace, targetEdit
    if vk = 32 && hwnd = targetEdit.Hwnd
        trace.Push({at:A_TickCount, message:message})
}
