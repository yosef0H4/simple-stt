class ShellLog {
    static maxBytes := 2 * 1024 * 1024
    static maxAgeSeconds := 7 * 24 * 60 * 60
    static ranks := Map("trace", 0, "debug", 1, "info", 2, "warning", 3, "error", 4)
    static thresholds := Map("extreme", 0, "debug", 1, "normal", 2, "minimal", 3)

    __New(path, level := "normal") {
        this.path := path
        this.pid := ProcessExist()
        this.SetLevel(level)
        SplitPath(path, , &dir)
        if dir != ""
            DirCreate(dir)
        this.PruneOldLog()
    }

    SetLevel(level) {
        this.level := StrLower(level . "")
    }

    ShouldWrite(level) {
        return ShellLog.ranks.Get(StrLower(level . ""), 2) >= ShellLog.thresholds.Get(this.level, 2)
    }

    Write(level, message, sessionId := "") {
        if !this.ShouldWrite(level)
            return
        stamp := FormatTime(, "yyyy-MM-dd'T'HH:mm:ss")
        line := stamp . " component=shell pid=" . this.pid . " level=" . level
        if sessionId != ""
            line .= " session_id=" . sessionId
        line .= " message=" . StrReplace(message, "`n", "\n") . "`n"
        try {
            this.PruneOldLog(StrLen(line) * 4)
            FileAppend(line, this.path, "UTF-8")
        }
    }

    PruneOldLog(incomingBytes := 0) {
        if !FileExist(this.path)
            return
        tooLarge := FileGetSize(this.path) + incomingBytes > ShellLog.maxBytes
        tooOld := DateDiff(A_Now, FileGetTime(this.path, "M"), "Seconds") > ShellLog.maxAgeSeconds
        if tooLarge || tooOld
            FileDelete(this.path)
    }
}
