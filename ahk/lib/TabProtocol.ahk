class TabProtocol {
    static Escape(value) {
        value := StrReplace(value . "", "\", "\\")
        value := StrReplace(value, "`t", "\t")
        value := StrReplace(value, "`r", "\r")
        return StrReplace(value, "`n", "\n")
    }

    static Unescape(value) {
        out := ""
        index := 1
        while index <= StrLen(value) {
            ch := SubStr(value, index, 1)
            if ch != "\" {
                out .= ch
                index += 1
                continue
            }
            next := SubStr(value, index + 1, 1)
            switch next {
                case "t": out .= "`t"
                case "r": out .= "`r"
                case "n": out .= "`n"
                case "\": out .= "\"
                default: out .= "\" . next
            }
            index += 2
        }
        return out
    }

    static ErrorResponse(message) {
        return Map("ok", false, "message", message, "values", Map(), "events", Array())
    }

    static ReadResponse(path) {
        if !FileExist(path)
            return this.ErrorResponse("helper did not create a response file")
        raw := ""
        Loop 20 {
            try {
                raw := FileRead(path, "UTF-8")
                break
            } catch Error as err {
                if A_Index = 20
                    return this.ErrorResponse("unable to read helper response after retry: " . err.Message)
                Sleep(10)
            }
        }
        response := Map("ok", false, "message", "", "values", Map(), "events", Array())
        eventBySeq := Map()
        for line in StrSplit(raw, "`n", "`r") {
            if line = ""
                continue
            parts := StrSplit(line, "`t")
            kind := parts[1]
            switch kind {
                case "status": response["ok"] := parts.Length >= 2 && parts[2] = "ok"
                case "message": response["message"] := parts.Length >= 2 ? this.Unescape(parts[2]) : ""
                case "value":
                    if parts.Length >= 3
                        response["values"][this.Unescape(parts[2])] := this.Unescape(parts[3])
                case "event":
                    if parts.Length >= 6 {
                        event := Map("seq", parts[2] + 0, "kind", this.Unescape(parts[3]), "session_id", parts[4], "level", parts[5], "text", this.Unescape(parts[6]), "values", Map())
                        response["events"].Push(event)
                        eventBySeq[parts[2]] := event
                    }
                case "event_value":
                    if parts.Length >= 4 && eventBySeq.Has(parts[2])
                        eventBySeq[parts[2]]["values"][this.Unescape(parts[3])] := this.Unescape(parts[4])
            }
        }
        return response
    }
}
