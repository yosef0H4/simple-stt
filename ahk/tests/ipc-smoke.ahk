#Requires AutoHotkey v2.0
#SingleInstance Force

#Include ..\lib\Utils.ahk
#Include ..\lib\TabProtocol.ahk
#Include ..\lib\Config.ahk

Fail(message, exitCode := 1) {
    SimpleSttConsoleError("FAIL: " . message)
    ExitApp(exitCode)
}

Info(message) {
    SimpleSttConsoleLine("INFO: " . message)
}

ctl := SimpleSttResolveExe("simple-stt-ctl")
capture := SimpleSttResolveExe("simple-stt-capture")
if !FileExist(ctl) || !FileExist(capture) {
    Fail("Build simple-stt-ctl.exe and simple-stt-capture.exe first.")
}
config := ConfigStore(ctl)
state := config.Get("service_state_path")
token := SimpleSttRandomToken()
try FileDelete(state)
Info("starting capture service")
Run(SimpleSttQuote(capture) . " --token " . SimpleSttQuote(token) . " --state-file " . SimpleSttQuote(state) . " --config " . SimpleSttQuote(config.Get("config_path")), A_ScriptDir, "Hide", &pid)
Loop 40 {
    Sleep(100)
    if FileExist(state)
        break
}
if !FileExist(state)
    Fail("capture state file was not published")
output := SimpleSttTempFile("ipc-smoke")
RunWait(SimpleSttQuote(ctl) . " --state-file " . SimpleSttQuote(state) . " --token " . SimpleSttQuote(token) . " --output " . SimpleSttQuote(output) . " ping", A_ScriptDir, "Hide")
response := TabProtocol.ReadResponse(output)
try FileDelete(output)
if !response["ok"] {
    try ProcessClose(pid)
    Fail("PING failed: " . response["message"])
}
if response["message"] != "pong" {
    ProcessClose(pid)
    Fail("unexpected ping response: " . response["message"])
}
output := SimpleSttTempFile("ipc-shutdown")
RunWait(SimpleSttQuote(ctl) . " --state-file " . SimpleSttQuote(state) . " --token " . SimpleSttQuote(token) . " --output " . SimpleSttQuote(output) . " shutdown", A_ScriptDir, "Hide")
try FileDelete(output)
try ProcessWaitClose(pid, 3)
SimpleSttConsoleLine("PASS: authenticated ping and graceful shutdown")
ExitApp(0)
