class Typist {
    static modifierKeys := ["LCtrl", "RCtrl", "LAlt", "RAlt", "LShift", "RShift", "LWin", "RWin"]

    __New(logger, onNotice) {
        this.logger := logger
        this.onNotice := onNotice
        this.active := false
        this.queue := Array()
        this.timer := ObjBindMethod(this, "Tick")
        this.clipboardBackup := ""
        this.pasteStage := 0
    }

    Begin(sessionId, targetWindow, text, pacedTypingEnabled, typingSpeedWpm, trailingSpace, deliveryMode := "type") {
        if deliveryMode != "type" && deliveryMode != "clipboard" && deliveryMode != "smart_paste" && deliveryMode != "paste_shift_insert" && deliveryMode != "paste_ctrl_v" && deliveryMode != "paste_ctrl_shift_v"
            deliveryMode := "type"
        item := Map(
            "session_id", sessionId,
            "target_window", targetWindow,
            "text", trailingSpace && text != "" ? text . " " : text,
            "paced_typing_enabled", !!pacedTypingEnabled,
            "typing_speed_wpm", Min(450, Max(50, typingSpeedWpm + 0)),
            "delivery_mode", deliveryMode
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
        this.offset := 1
        this.burstFactor := 1.0
        this.burstRemaining := 0
        this.pasteStage := 0
        this.pasteClipboardSequence := 0
        this.clipboardBackup := ""
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
        if this.pasteStage = 0 {
            try {
                if this.deliveryMode = "clipboard" {
                    A_Clipboard := this.text
                    if !ClipWait(1)
                        throw Error("clipboard text did not become available")
                    this.CompleteCurrent()
                    return
                }
                this.clipboardBackup := ClipboardAll()
                A_Clipboard := ""
                A_Clipboard := this.text
                if !ClipWait(1)
                    throw Error("clipboard text did not become available")
                this.pasteClipboardSequence := DllCall("user32\GetClipboardSequenceNumber", "UInt")
                ; Give Windows a moment to publish the new clipboard payload before
                ; the target application receives the paste shortcut.
                Sleep(60)
                if this.deliveryMode = "paste_shift_insert"
                    Send("+{Insert}")
                else if this.deliveryMode = "paste_ctrl_shift_v"
                    Send("^+v")
                else
                    Send("^v")
                this.pasteStage := 1
                ; Some target controls process WM_PASTE asynchronously after the
                ; shortcut returns. Keep the temporary text on the clipboard long
                ; enough for slower apps, then restore the user's full clipboard.
                SetTimer(this.timer, -400)
                return
            } catch Error as err {
                this.RestoreClipboardIfOwned()
                this.CancelCurrent("Paste failed: " . err.Message, true)
                this.StartNext()
                return
            }
        }
        this.RestoreClipboardIfOwned()
        this.CompleteCurrent()
    }

    CompleteCurrent() {
        this.logger.Write("info", "text-delivery success mode=" . this.deliveryMode, this.sessionId)
        this.active := false
        this.pasteStage := 0
        this.clipboardBackup := ""
        this.StartNext()
    }

    RestoreClipboardIfOwned() {
        if !IsObject(this.clipboardBackup)
            return
        currentSequence := DllCall("user32\GetClipboardSequenceNumber", "UInt")
        if this.pasteClipboardSequence = 0 || currentSequence = this.pasteClipboardSequence {
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
        this.RestoreClipboardIfOwned()
        this.logger.Write("warning", reason, this.sessionId)
        this.active := false
        this.pasteStage := 0
        if notify && IsObject(this.onNotice)
            this.onNotice.Call(reason, "warning")
    }

    Cancel(reason := "text delivery cancelled", notify := false, clearQueue := true) {
        this.CancelCurrent(reason, notify)
        if clearQueue && this.queue.Length {
            this.logger.Write("warning", "text-delivery queue cleared count=" . this.queue.Length)
            this.queue := Array()
        }
    }

    AnyPhysicalModifierDown() {
        for key in Typist.modifierKeys {
            if GetKeyState(key, "P")
                return true
        }
        return false
    }
}
