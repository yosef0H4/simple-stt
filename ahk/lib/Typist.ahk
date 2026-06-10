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

    Begin(sessionId, targetWindow, text, chunkChars, intervalMs, trailingSpace, deliveryMode := "type") {
        if deliveryMode != "type" && deliveryMode != "paste_ctrl_v" && deliveryMode != "paste_ctrl_shift_v"
            deliveryMode := "type"
        item := Map(
            "session_id", sessionId,
            "target_window", targetWindow,
            "text", trailingSpace && text != "" ? text . " " : text,
            "chunk_chars", Max(1, chunkChars + 0),
            "interval_ms", Max(0, intervalMs + 0),
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
        this.chunkChars := item["chunk_chars"]
        this.intervalMs := item["interval_ms"]
        this.deliveryMode := item["delivery_mode"]
        this.offset := 1
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
        chunk := SubStr(this.text, this.offset, this.chunkChars)
        try SendText(chunk)
        catch Error as err {
            this.CancelCurrent("SendText failed: " . err.Message, true)
            this.StartNext()
            return
        }
        this.offset += StrLen(chunk)
        SetTimer(this.timer, this.intervalMs > 0 ? -this.intervalMs : -1)
    }

    TickPaste() {
        if this.pasteStage = 0 {
            try {
                this.clipboardBackup := ClipboardAll()
                A_Clipboard := ""
                A_Clipboard := this.text
                if !ClipWait(1)
                    throw Error("clipboard text did not become available")
                this.pasteClipboardSequence := DllCall("user32\GetClipboardSequenceNumber", "UInt")
                ; Give Windows a moment to publish the new clipboard payload before
                ; the target application receives the paste shortcut.
                Sleep(60)
                if this.deliveryMode = "paste_ctrl_shift_v"
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
