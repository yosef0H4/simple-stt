#Requires AutoHotkey v2.0
#SingleInstance Force

; This helper only replays a validated, explicitly saved keyboard action.
keysHeld := Map()
heldFile := ""
clockFrequency := 0
timerActive := false
OnExit(ReleaseHeld)
try {
    if A_Args.Length != 4 && A_Args.Length != 5
        throw Error("Invalid action arguments")
    heldFile := A_Args[3] ".held"
    if A_Args.Length == 5 {
        if A_Args[5] != "release"
            throw Error("Invalid cleanup arguments")
        if FileExist(heldFile)
            for scan in StrSplit(Trim(FileRead(heldFile, "UTF-8"), "`r`n"), "`n", "`r")
                if RegExMatch(scan, "^[0-9A-F]{3}$")
                    keysHeld[scan] := true
        ReleaseHeld()
        ExitApp(0)
    }
    targetHwnd := Integer(A_Args[1])
    targetPid := Integer(A_Args[2])
    cancelFile := A_Args[4]
    events := []
    previous := 0
    for line in StrSplit(Trim(FileRead(A_Args[3], "UTF-8"), "`r`n"), "`n", "`r") {
        parts := StrSplit(line, "`t")
        if parts.Length != 3
            throw Error("Invalid action event")
        at := Integer(parts[1]), scan := Integer(parts[2]), down := Integer(parts[3])
        if at < previous || at > 30000 || scan < 1 || scan > 511 || (down != 0 && down != 1)
            throw Error("Invalid action event")
        events.Push({at: at, scan: Format("{:03X}", scan), down: down})
        previous := at
    }
    if events.Length < 2 || events.Length > 128 || events[1].at != 0
        throw Error("Invalid action length")
    InstallKeybdHook()
    for modifier in ["LControl", "RControl", "LShift", "RShift", "LAlt", "RAlt", "LWin", "RWin"]
        if GetKeyState(modifier, "P")
            throw Error("Release physical modifiers before replay")
    CheckTarget()
    DllCall("QueryPerformanceFrequency", "Int64*", &clockFrequency)
    timerActive := DllCall("winmm\timeBeginPeriod", "UInt", 1, "UInt") = 0
    started := NowMs()
    for event in events {
        loop {
            CheckTarget()
            remaining := event.at - (NowMs() - started)
            if remaining <= 0
                break
            DllCall("Sleep", "UInt", Max(1, Floor(Min(remaining, 5))))
        }
        if event.down {
            keysHeld[event.scan] := true
            SaveHeld()
        }
        SendInput("{Blind}{sc" event.scan (event.down ? " down}" : " up}"))
        if !event.down && keysHeld.Has(event.scan) {
            keysHeld.Delete(event.scan)
            SaveHeld()
        }
    }
    ReleaseHeld()
    ExitApp(0)
} catch {
    ReleaseHeld()
    FileAppend("Recorded action stopped or target unavailable.`n", "**")
    ExitApp(2)
}

CheckTarget() {
    global targetHwnd, targetPid, cancelFile
    if FileExist(cancelFile) || !WinExist("ahk_id " targetHwnd) || WinGetPID("ahk_id " targetHwnd) != targetPid
        throw Error("Action canceled or target closed")
    root := DllCall("GetAncestor", "ptr", targetHwnd, "uint", 2, "ptr")
    if !WinActive("ahk_id " (root ? root : targetHwnd))
        throw Error("Target focus changed")
}

ReleaseHeld(*) {
    global keysHeld, heldFile, timerActive
    for scan in keysHeld {
        try SendInput("{Blind}{sc" scan " up}")
    }
    keysHeld.Clear()
    if timerActive {
        DllCall("winmm\timeEndPeriod", "UInt", 1)
        timerActive := false
    }
    if heldFile && FileExist(heldFile)
        try FileDelete(heldFile)
}

SaveHeld() {
    global keysHeld, heldFile
    contents := ""
    for scan in keysHeld
        contents .= scan "`n"
    temporary := heldFile ".new"
    if FileExist(temporary)
        FileDelete(temporary)
    FileAppend(contents, temporary, "UTF-8-RAW")
    FileMove(temporary, heldFile, 1)
}

NowMs() {
    global clockFrequency
    counter := 0
    DllCall("QueryPerformanceCounter", "Int64*", &counter)
    return counter * 1000 / clockFrequency
}
