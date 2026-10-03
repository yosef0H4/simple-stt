class Typist {
    static modifierKeys := ["LCtrl", "RCtrl", "LAlt", "RAlt", "LShift", "RShift", "LWin", "RWin"]

    __New(logger, onNotice, onFinished := "") {
        this.logger := logger
        this.onNotice := onNotice
        this.onFinished := onFinished
        this.active := false
        this.queue := Array()
        this.timer := ObjBindMethod(this, "Tick")
        this.clipboardBackup := ""
        this.pasteStage := 0
        this.pasteProbe := ""
        this.pasteExpected := ""
        this.pasteVerified := false
    }

    Begin(sessionId, targetWindow, text, pacedTypingEnabled, typingSpeedWpm, trailingSpace, deliveryMode := "type", cleanClipboardOnPaste := true) {
        if deliveryMode != "type" && deliveryMode != "clipboard" && deliveryMode != "smart_paste" && deliveryMode != "paste_shift_insert" && deliveryMode != "paste_ctrl_v" && deliveryMode != "paste_ctrl_shift_v"
            deliveryMode := "type"
        item := Map(
            "session_id", sessionId,
            "target_window", targetWindow,
            "text", trailingSpace && text != "" ? text . " " : text,
            "paced_typing_enabled", !!pacedTypingEnabled,
            "typing_speed_wpm", Min(850, Max(50, typingSpeedWpm + 0)),
            "delivery_mode", deliveryMode,
            "clean_clipboard_on_paste", !!cleanClipboardOnPaste
        )
        if this.active {
            this.queue.Push(item)
            this.logger.Write("info", "text-delivery queued chars=" . StrLen(item["text"]) . " queue_depth=" . this.queue.Length, sessionId)
            return
        }
        this.StartItem(item)
    }

    StartItem(item) {
        this.sessionId := item["session_id"]
        this.targetWindow := item["target_window"]
        this.text := item["text"]
        this.textLength := StrLen(this.text)
        this.pacedTypingEnabled := item["paced_typing_enabled"]
        this.typingSpeedWpm := item["typing_speed_wpm"]
        this.deliveryMode := item["delivery_mode"]
        this.cleanClipboardOnPaste := item["clean_clipboard_on_paste"]
        this.offset := 1
        this.burstFactor := 1.0
        this.burstRemaining := 0
        this.pasteStage := 0
        this.pasteClipboardSequence := 0
        this.clipboardBackup := ""
        this.pasteProbe := ""
        this.pasteExpected := ""
        this.pasteVerified := false
        this.active := true
        this.logger.Write("info", "text-delivery begin mode=" . this.deliveryMode . " chars=" . this.textLength, this.sessionId)
        SetTimer(this.timer, -1)
    }

    StartNext() {
        if this.active || this.queue.Length = 0
            return
        this.StartItem(this.queue.RemoveAt(1))
    }

    Tick(*) {
        if !this.active
            return
        if WinActive("A") != this.targetWindow {
            this.CancelCurrent("foreground-window mismatch; transcript was not delivered", true)
            this.StartNext()
            return
        }
        if this.AnyPhysicalModifierDown() {
            SetTimer(this.timer, -25)
            return
        }
        if this.deliveryMode = "type"
            this.TickType()
        else
            this.TickPaste()
    }

    TickType() {
        if this.offset > this.textLength {
            this.CompleteCurrent()
            return
        }
        if !this.pacedTypingEnabled {
            try SendText(SubStr(this.text, this.offset))
            catch Error as err {
                this.CancelCurrent("SendText failed: " . err.Message, true)
                this.StartNext()
                return
            }
            this.offset := this.textLength + 1
            this.CompleteCurrent()
            return
        }
        currentChar := SubStr(this.text, this.offset, 1)
        try SendText(currentChar)
        catch Error as err {
            this.CancelCurrent("SendText failed: " . err.Message, true)
            this.StartNext()
            return
        }
        previousChar := this.offset > 1 ? SubStr(this.text, this.offset - 1, 1) : ""
        nextChar := this.offset < this.textLength ? SubStr(this.text, this.offset + 1, 1) : ""
        this.offset += 1
        if this.offset > this.textLength {
            this.CompleteCurrent()
            return
        }
        SetTimer(this.timer, -this.TypingDelay(previousChar, currentChar, nextChar))
    }

    TypingDelay(previousChar, currentChar, nextChar) {
        baseDelay := 12000 / Max(this.typingSpeedWpm, 1)
        if this.burstRemaining <= 0 {
            this.burstFactor := Random(0.93, 1.07)
            this.burstRemaining := Random(4, 11)
        }
        this.burstRemaining -= 1
        jitter := (Random(0.82, 1.18) + Random(0.82, 1.18) + Random(0.82, 1.18)) / 3
        delay := baseDelay * this.burstFactor * jitter
        delay *= this.TransitionFactor(currentChar, nextChar)
        delay *= this.BoundaryFactor(previousChar, currentChar)
        return Round(Max(delay, 15))
    }

    TransitionFactor(previousChar, currentChar) {
        previousFinger := this.GetFinger(previousChar)
        currentFinger := this.GetFinger(currentChar)
        if previousFinger = "" || currentFinger = ""
            return 1.0
        if StrLower(previousChar) = StrLower(currentChar)
            return Random(0.78, 0.90)
        if SubStr(previousFinger, 1, 1) != SubStr(currentFinger, 1, 1)
            return Random(0.86, 0.96)
        if previousFinger = currentFinger
            return Random(1.10, 1.25)
        return Random(0.98, 1.08)
    }

    BoundaryFactor(previousChar, currentChar) {
        if currentChar = "`n"
            return Random(1.70, 2.30)
        if currentChar = "`t"
            return Random(1.15, 1.40)
        if currentChar = " " {
            if InStr(".!?", previousChar)
                return Random(1.55, 2.15)
            if InStr(",;:", previousChar)
                return Random(1.15, 1.45)
            return Random(0.96, 1.08)
        }
        return 1.0
    }

    GetFinger(char) {
        if char = ""
            return ""
        char := StrLower(char)
        for entry in [["qaz", "L1"], ["wsx", "L2"], ["edc", "L3"], ["rfvtgb", "L4"], ["yuhjnm", "R4"], ["ik", "R3"], ["ol", "R2"], ["p", "R1"]] {
            if InStr(entry[1], char)
                return entry[2]
        }
        return ""
    }

    TickPaste() {
        if this.text == "" {
            this.CompleteCurrent("skipped_empty")
            return
        }
        operationSession := this.sessionId
        if this.pasteStage = 0 {
            try {
                if this.deliveryMode = "clipboard" {
                    A_Clipboard := this.text
                    clipboardSequence := DllCall("user32\GetClipboardSequenceNumber", "UInt")
                    if !ClipWait(1)
                        throw Error("clipboard text did not become available")
                    if !this.active || this.sessionId != operationSession
                        return
                    if DllCall("user32\GetClipboardSequenceNumber", "UInt") != clipboardSequence || !(A_Clipboard == this.text)
                        throw Error("clipboard changed before clipboard delivery completed")
                    this.CompleteCurrent()
                    return
                }
                this.clipboardBackup := ClipboardAll()
                if this.cleanClipboardOnPaste {
                    clipboardSequence := this.PublishCleanClipboardText(this.text, operationSession)
                    if !this.active || this.sessionId != operationSession
                        return
                    this.pasteClipboardSequence := clipboardSequence
                } else {
                    A_Clipboard := this.text
                    this.pasteClipboardSequence := DllCall("user32\GetClipboardSequenceNumber", "UInt")
                }
                if !ClipWait(1)
                    throw Error("clipboard text did not become available")
                if !this.active || this.sessionId != operationSession
                    return
                if WinActive("A") != this.targetWindow
                    throw Error("foreground window changed before paste")
                if DllCall("user32\GetClipboardSequenceNumber", "UInt") != this.pasteClipboardSequence || !(A_Clipboard == this.text)
                    throw Error("clipboard changed before paste")
                this.pasteProbe := this.CaptureEditPasteProbe(this.targetWindow, this.text)
                this.pasteExpected := IsObject(this.pasteProbe) ? this.pasteProbe["expected"] : ""
                if !this.active || this.sessionId != operationSession
                    return
                if WinActive("A") != this.targetWindow || DllCall("user32\GetClipboardSequenceNumber", "UInt") != this.pasteClipboardSequence || !(A_Clipboard == this.text)
                    throw Error("paste target or clipboard changed before injection")
                ; Mark before injecting: cancellation after this point must retain
                ; the temporary clipboard unless a standard Edit proves insertion.
                this.pasteStage := 1
                if this.deliveryMode = "paste_shift_insert"
                    SendEvent("{Shift down}{Insert}{Shift up}")
                else if this.deliveryMode = "paste_ctrl_shift_v"
                    Send("^+v")
                else
                    Send("^v")
                this.pasteDeadline := A_TickCount + 3000
                SetTimer(this.timer, -25)
                return
            } catch Error as err {
                if this.active && this.sessionId = operationSession {
                    this.RestoreClipboardIfOwned()
                    this.CancelCurrent("Paste failed: " . err.Message, true)
                    this.StartNext()
                }
                return
            }
        }
        if IsObject(this.pasteProbe) {
            currentText := this.ReadEditText(this.pasteProbe["hwnd"])
            if IsObject(currentText) && currentText["ok"] && currentText["text"] == this.pasteExpected {
                this.pasteVerified := true
                this.RestoreClipboardIfOwned()
                this.CompleteCurrent("confirmed")
                return
            }
            if A_TickCount < this.pasteDeadline {
                SetTimer(this.timer, -25)
                return
            }
        }
        this.CompleteCurrent("submitted_unverified")
    }

    ; Publishes Unicode text and Windows clipboard-history exclusion markers in
    ; one clipboard transaction. SetClipboardData takes ownership of successful
    ; movable HGLOBAL handles; failed handles remain ours to free.
    PublishCleanClipboardText(text, operationSession) {
        formats := Map()
        formats[13] := this.AllocClipboardBlock(text . "`0", (StrLen(text) + 1) * 2)
        formats[DllCall("user32\RegisterClipboardFormatW", "Str", "ExcludeClipboardContentFromMonitorProcessing", "UInt")] := this.AllocClipboardDword(0)
        formats[DllCall("user32\RegisterClipboardFormatW", "Str", "CanIncludeInClipboardHistory", "UInt")] := this.AllocClipboardDword(0)
        formats[DllCall("user32\RegisterClipboardFormatW", "Str", "CanUploadToCloudClipboard", "UInt")] := this.AllocClipboardDword(0)
        for format, handle in formats {
            if !format || !handle {
                this.FreeClipboardBlocks(formats)
                throw Error("unable to allocate clipboard text or history markers")
            }
        }
        try {
            Loop 10 {
                if !this.active || this.sessionId != operationSession
                    throw Error("paste cancelled before clipboard publication")
                priorCritical := A_IsCritical
                Critical("On")
                opened := false
                try {
                    ; Recheck after entering Critical so an older operation
                    ; cannot publish after a newer session supersedes it.
                    if !this.active || this.sessionId != operationSession
                        throw Error("paste cancelled before clipboard publication")
                    if DllCall("user32\OpenClipboard", "Ptr", A_ScriptHwnd, "Int") {
                        opened := true
                        if !DllCall("user32\EmptyClipboard", "Int")
                            throw Error("unable to empty clipboard")
                        for format, handle in formats {
                            if !DllCall("user32\SetClipboardData", "UInt", format, "Ptr", handle, "Ptr")
                                throw Error("unable to publish clipboard format " . format)
                            formats[format] := 0
                        }
                        DllCall("user32\CloseClipboard")
                        opened := false
                        return DllCall("user32\GetClipboardSequenceNumber", "UInt")
                    }
                } finally {
                    if opened
                        DllCall("user32\CloseClipboard")
                    Critical(priorCritical)
                }
                Sleep(10)
            }
            throw Error("clipboard remained busy")
        } finally {
            this.FreeClipboardBlocks(formats)
        }
    }

    AllocClipboardBlock(value, byteCount) {
        handle := DllCall("kernel32\GlobalAlloc", "UInt", 0x42, "UPtr", byteCount, "Ptr") ; GMEM_MOVEABLE | GMEM_ZEROINIT
        if !handle
            return 0
        pointer := DllCall("kernel32\GlobalLock", "Ptr", handle, "Ptr")
        if !pointer {
            DllCall("kernel32\GlobalFree", "Ptr", handle, "Ptr")
            return 0
        }
        StrPut(value, pointer, byteCount // 2, "UTF-16")
        DllCall("kernel32\GlobalUnlock", "Ptr", handle)
        return handle
    }

    AllocClipboardDword(value) {
        handle := DllCall("kernel32\GlobalAlloc", "UInt", 0x42, "UPtr", 4, "Ptr") ; GMEM_MOVEABLE | GMEM_ZEROINIT
        if !handle
            return 0
        pointer := DllCall("kernel32\GlobalLock", "Ptr", handle, "Ptr")
        if !pointer {
            DllCall("kernel32\GlobalFree", "Ptr", handle, "Ptr")
            return 0
        }
        NumPut("UInt", value, pointer)
        DllCall("kernel32\GlobalUnlock", "Ptr", handle)
        return handle
    }

    FreeClipboardBlocks(formats) {
        for _, handle in formats {
            if handle
                DllCall("kernel32\GlobalFree", "Ptr", handle, "Ptr")
        }
    }

    CompleteCurrent(outcome := "success") {
        this.logger.Write("info", "text-delivery " . outcome . " mode=" . this.deliveryMode, this.sessionId)
        completedSession := this.sessionId
        this.active := false
        this.pasteStage := 0
        this.clipboardBackup := ""
        this.NotifyFinished(completedSession)
        this.StartNext()
    }

    RestoreClipboardIfOwned() {
        if !IsObject(this.clipboardBackup)
            return
        if this.pasteStage = 1 && !this.pasteVerified
            return
        if this.pasteStage = 1 && this.pasteVerified && !this.cleanClipboardOnPaste {
            this.clipboardBackup := ""
            return
        }
        currentSequence := DllCall("user32\GetClipboardSequenceNumber", "UInt")
        if this.pasteClipboardSequence != 0 && currentSequence = this.pasteClipboardSequence {
            try A_Clipboard := this.clipboardBackup
            catch Error as err
                this.logger.Write("warning", "clipboard restore failed: " . err.Message, this.sessionId)
        } else {
            this.logger.Write("warning", "clipboard changed during paste; skipped restore", this.sessionId)
        }
        this.clipboardBackup := ""
    }

    CancelCurrent(reason := "text delivery cancelled", notify := false) {
        if !this.active
            return
        SetTimer(this.timer, 0)
        if this.pasteStage = 0
            this.RestoreClipboardIfOwned()
        else if IsObject(this.pasteProbe) {
            currentText := this.ReadEditText(this.pasteProbe["hwnd"])
            if IsObject(currentText) && currentText["ok"] && currentText["text"] == this.pasteExpected {
                this.pasteVerified := true
                this.RestoreClipboardIfOwned()
            }
        }
        this.logger.Write("warning", reason, this.sessionId)
        cancelledSession := this.sessionId
        this.active := false
        this.pasteStage := 0
        this.NotifyFinished(cancelledSession)
        if notify && IsObject(this.onNotice)
            this.onNotice.Call(reason, "warning")
    }

    Cancel(reason := "text delivery cancelled", notify := false, clearQueue := true) {
        this.CancelCurrent(reason, notify)
        if clearQueue && this.queue.Length {
            this.logger.Write("warning", "text-delivery queue cleared count=" . this.queue.Length)
            for item in this.queue
                this.NotifyFinished(item["session_id"])
            this.queue := Array()
        }
    }

    NotifyFinished(sessionId) {
        if IsObject(this.onFinished)
            this.onFinished.Call(sessionId)
    }

    AnyPhysicalModifierDown() {
        for key in Typist.modifierKeys {
            if GetKeyState(key, "P")
                return true
        }
        return false
    }

    CaptureEditPasteProbe(hwnd, insertText) {
        try focusedControl := ControlGetFocus("ahk_id " . hwnd)
        catch
            return ""
        if focusedControl = ""
            return ""
        try focusedHwnd := ControlGetHwnd(focusedControl, "ahk_id " . hwnd)
        catch
            return ""
        className := Buffer(512, 0)
        if !DllCall("user32\GetClassNameW", "Ptr", focusedHwnd, "Ptr", className, "Int", 256, "Int") || StrGet(className, "UTF-16") != "Edit"
            return ""
        style := DllCall("user32\GetWindowLongPtrW", "Ptr", focusedHwnd, "Int", -16, "Ptr")
        if style & 0x20 ; ES_PASSWORD
            return ""
        current := this.ReadEditText(focusedHwnd)
        if !IsObject(current) || !current["ok"]
            return ""
        startPos := Buffer(4, 0), endPos := Buffer(4, 0), result := Buffer(A_PtrSize, 0)
        if !this.SendMessageBounded(focusedHwnd, 0x00B0, startPos.Ptr, endPos.Ptr, result.Ptr) ; EM_GETSEL
            return ""
        start := NumGet(startPos, 0, "UInt"), finish := NumGet(endPos, 0, "UInt")
        normalized := StrReplace(insertText, "`r`n", "`n")
        normalized := StrReplace(normalized, "`n", "`r`n")
        expected := SubStr(current["text"], 1, start) . normalized . SubStr(current["text"], finish + 1)
        ; An unchanged value is not evidence that the target consumed a paste.
        if expected == current["text"]
            return ""
        return Map("hwnd", focusedHwnd, "expected", expected)
    }

    ReadEditText(hwnd) {
        lengthResult := Buffer(A_PtrSize, 0)
        if !this.SendMessageBounded(hwnd, 0x000E, 0, 0, lengthResult.Ptr) ; WM_GETTEXTLENGTH
            return Map("ok", false, "text", "")
        length := NumGet(lengthResult, 0, "Ptr")
        if length > 65535
            return Map("ok", false, "text", "")
        textBuffer := Buffer((length + 1) * 2, 0), copied := Buffer(A_PtrSize, 0)
        if !this.SendMessageBounded(hwnd, 0x000D, length + 1, textBuffer.Ptr, copied.Ptr) ; WM_GETTEXT
            return Map("ok", false, "text", "")
        return Map("ok", true, "text", StrGet(textBuffer, NumGet(copied, 0, "Ptr"), "UTF-16"))
    }

    SendMessageBounded(hwnd, message, wParam, lParam, resultPtr) {
        return !!DllCall("user32\SendMessageTimeoutW", "Ptr", hwnd, "UInt", message, "UPtr", wParam, "Ptr", lParam, "UInt", 0x2, "UInt", 1000, "Ptr", resultPtr, "Ptr")
    }
}
