class Typist {
    __New(logger, onNotice) {
        this.logger := logger
        this.onNotice := onNotice
        this.active := false
        this.queue := Array()
        this.timer := ObjBindMethod(this, "Tick")
    }

    Begin(sessionId, targetWindow, text, chunkChars, intervalMs, trailingSpace) {
        item := Map(
            "session_id", sessionId,
            "target_window", targetWindow,
            "text", trailingSpace && text != "" ? text . " " : text,
            "chunk_chars", Max(1, chunkChars + 0),
            "interval_ms", Max(0, intervalMs + 0)
        )
        if this.active {
            this.queue.Push(item)
            this.logger.Write("info", "typed-text queued chars=" . StrLen(item["text"]) . " queue_depth=" . this.queue.Length, sessionId)
            return
        }
        this.StartItem(item)
    }

    StartItem(item) {
        this.sessionId := item["session_id"]
        this.targetWindow := item["target_window"]
        this.text := item["text"]
        this.chunkChars := item["chunk_chars"]
        this.intervalMs := item["interval_ms"]
        this.offset := 1
        this.active := true
        this.logger.Write("info", "typed-text begin chars=" . StrLen(this.text), this.sessionId)
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
            this.CancelCurrent("foreground-window mismatch; transcript was not typed", true)
            this.StartNext()
            return
        }
        if this.AnyPhysicalModifierDown() {
            SetTimer(this.timer, -25)
            return
        }
        if this.offset > StrLen(this.text) {
            this.logger.Write("info", "typed-text success", this.sessionId)
            this.active := false
            this.StartNext()
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

    CancelCurrent(reason := "typing cancelled", notify := false) {
        if !this.active
            return
        SetTimer(this.timer, 0)
        this.logger.Write("warning", reason, this.sessionId)
        this.active := false
        if notify && IsObject(this.onNotice)
            this.onNotice.Call(reason, "warning")
    }

    Cancel(reason := "typing cancelled", notify := false, clearQueue := true) {
        this.CancelCurrent(reason, notify)
        if clearQueue && this.queue.Length {
            this.logger.Write("warning", "typed-text queue cleared count=" . this.queue.Length)
            this.queue := Array()
        }
    }

    AnyPhysicalModifierDown() {
        for key in ["LCtrl", "RCtrl", "LAlt", "RAlt", "LShift", "RShift", "LWin", "RWin"] {
            if GetKeyState(key, "P")
                return true
        }
        return false
    }
}
