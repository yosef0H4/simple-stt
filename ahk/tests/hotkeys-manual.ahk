#Requires AutoHotkey v2.0
#SingleInstance Force

#Include ..\lib\Utils.ahk
#Include ..\lib\Logging.ahk
#Include ..\lib\Hotkeys.ahk

global DownCount := 0
global UpCount := 0

Down() {
    global DownCount
    DownCount += 1
}

Up() {
    global UpCount
    UpCount += 1
}

Fail(message, exitCode := 1) {
    UvoxConsoleError("FAIL: " . message)
    ExitApp(exitCode)
}

logger := ShellLog(A_Temp . "\uvox-hotkeys-manual.log")
manager := HotkeyManager(Down, Up, logger)
manager.Configure("CapsLock+S", true, "preserve_tap")
if !manager.enabled
    Fail("manager should be enabled after Configure()")

for label in ["CapsLock+S", "LCtrl+S", "AltGr+S", "RAlt+S", "LShift+X"] {
    spec := HotkeySpec.Parse(label)
    if !spec.Has("down") || !spec.Has("up")
        Fail("parsed hotkey spec is incomplete for " . label)
}

manager.Configure("LCtrl+S", true, "preserve_tap")
manager.SetEnabled(false)
if manager.enabled
    Fail("manager should be disabled after SetEnabled(false)")
manager.SetEnabled(true)
if !manager.enabled
    Fail("manager should be enabled after SetEnabled(true)")
manager.Configure("AltGr+S", true, "preserve_tap")
manager.DisableBindings()

UvoxConsoleLine("PASS: hotkey parser and binding smoke")
ExitApp(0)
