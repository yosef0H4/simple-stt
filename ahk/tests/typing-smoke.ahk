#Requires AutoHotkey v2.0
#SingleInstance Force

#Include ..\lib\Utils.ahk
#Include ..\lib\Logging.ahk
#Include ..\lib\Typist.ahk

logger := ShellLog(A_Temp . "\simple-stt-typing-smoke.log")
typistInstance := Typist(logger, Notice)

Notice(text, level := "info") {
    SimpleSttConsoleLine("NOTICE[" . level . "]: " . text)
}

Fail(message, exitCode := 1) {
    SimpleSttConsoleError("FAIL: " . message)
    ExitApp(exitCode)
}

if typistInstance.active
    Fail("typist should start inactive")

; Use a target window that cannot match the foreground window so the timer-driven
; path cancels before SendText and remains safe for headless runs.
if typistInstance.GetFinger("م") != "" || typistInstance.GetFinger("界") != "" || typistInstance.GetFinger("🙂") != ""
    Fail("non-Latin characters must use the language-neutral timing path")
if typistInstance.TransitionFactor("م", "ر") != 1.0
    Fail("non-Latin transitions must preserve Unicode with neutral timing")

typistInstance.Begin(1, -1, "Unicode: مرحبا 世界 🙂 — safe typing", true, 50, true)
typistInstance.Begin(2, -1, "Queued text", true, 50, true)
Sleep(150)

if typistInstance.active
    Fail("typist should have cancelled the active item in headless mode")
if typistInstance.queue.Length != 0
    Fail("expected the queued transcript to drain after cancellation, got queue length " . typistInstance.queue.Length)

typistInstance.Cancel("test cleanup", false, true)
if typistInstance.queue.Length != 0
    Fail("typist queue was not cleared by Cancel()")

typistInstance.Begin(3, -1, "Cancel me", false, 50, true)
typistInstance.Begin(4, -1, "Queued cancel", false, 50, true)
typistInstance.Cancel("global cancel smoke", false, true)
if typistInstance.active
    Fail("typist should be inactive after explicit cancel")
if typistInstance.queue.Length != 0
    Fail("typist explicit cancel did not clear queue")

SimpleSttConsoleLine("PASS: typist queue/cancel headless smoke")
ExitApp(0)
