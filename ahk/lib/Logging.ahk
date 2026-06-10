class ShellLog {
    static ranks := Map("trace", 0, "debug", 1, "info", 2, "warning", 3, "error", 4)
    static thresholds := Map("extreme", 0, "debug", 1, "normal", 2, "minimal", 3)

    __New(path, level := "normal") {
        this.path := path
        this.pid := ProcessExist()
        this.SetLevel(level)
        SplitPath(path, , &dir)
        if dir != ""
            DirCreate(dir)
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
        try FileAppend(line, this.path, "UTF-8")
    }
}
