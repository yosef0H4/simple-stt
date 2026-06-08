#Requires AutoHotkey v2.0
#SingleInstance Force

#Include ..\lib\Utils.ahk
#Include ..\lib\TabProtocol.ahk
#Include ..\lib\Config.ahk

Fail(message, exitCode := 1) {
    UvoxConsoleError("FAIL: " . message)
    ExitApp(exitCode)
}

Info(message) {
    UvoxConsoleLine("INFO: " . message)
}

ctl := UvoxResolveExe("uvoxctl")
capture := UvoxResolveExe("uvox-capture")
if !FileExist(ctl) || !FileExist(capture) {
    Fail("Build uvoxctl.exe and uvox-capture.exe first.")
}
config := ConfigStore(ctl)
state := config.Get("service_state_path")
token := UvoxRandomToken()
try FileDelete(state)
Info("starting capture service")
Run(UvoxQuote(capture) . " --token " . UvoxQuote(token) . " --state-file " . UvoxQuote(state) . " --config " . UvoxQuote(config.Get("config_path")), A_ScriptDir, "Hide", &pid)
Loop 40 {
    Sleep(100)
    if FileExist(state)
        break
}
if !FileExist(state)
    Fail("capture state file was not published")
output := UvoxTempFile("ipc-smoke")
RunWait(UvoxQuote(ctl) . " --state-file " . UvoxQuote(state) . " --token " . UvoxQuote(token) . " --output " . UvoxQuote(output) . " ping", A_ScriptDir, "Hide")
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
output := UvoxTempFile("ipc-shutdown")
RunWait(UvoxQuote(ctl) . " --state-file " . UvoxQuote(state) . " --token " . UvoxQuote(token) . " --output " . UvoxQuote(output) . " shutdown", A_ScriptDir, "Hide")
try FileDelete(output)
try ProcessWaitClose(pid, 3)
UvoxConsoleLine("PASS: authenticated ping and graceful shutdown")
ExitApp(0)
