#Requires AutoHotkey v2.0
#SingleInstance Force

#Include ..\lib\Utils.ahk
#Include ..\lib\TabProtocol.ahk

Fail(message, exitCode := 1) {
    SimpleSttConsoleError("FAIL: " . message)
    ExitApp(exitCode)
}

path := SimpleSttTempFile("tabprotocol-retry")
FileAppend("status`tok`nmessage`tpong`n", path, "UTF-8-RAW")
handle := DllCall("kernel32\CreateFileW", "Str", path, "UInt", 0x80000000, "UInt", 0, "Ptr", 0, "UInt", 3, "UInt", 0x80, "Ptr", 0, "Ptr")
if handle = -1
    Fail("unable to open exclusive retry fixture")

SetTimer((*) => DllCall("kernel32\CloseHandle", "Ptr", handle), -80)
response := TabProtocol.ReadResponse(path)
try FileDelete(path)
if !response["ok"] || response["message"] != "pong"
    Fail("retry read did not parse response")

SimpleSttConsoleLine("PASS: TabProtocol sharing-violation retry smoke")
ExitApp(0)
