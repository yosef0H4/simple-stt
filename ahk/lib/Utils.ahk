SimpleSttQuote(value) {
    return Chr(34) . value . Chr(34)
}

SimpleSttResolveExe(stem) {
    sameDir := A_ScriptDir . "\" . stem . ".exe"
    if FileExist(sameDir)
        return sameDir
    parentDir := A_ScriptDir . "\..\" . stem . ".exe"
    if FileExist(parentDir)
        return parentDir
    release := A_ScriptDir . "\..\target\release\" . stem . ".exe"
    if FileExist(release)
        return release
    releaseFromParent := A_ScriptDir . "\..\..\target\release\" . stem . ".exe"
    if FileExist(releaseFromParent)
        return releaseFromParent
    debug := A_ScriptDir . "\..\target\debug\" . stem . ".exe"
    if FileExist(debug)
        return debug
    debugFromParent := A_ScriptDir . "\..\..\target\debug\" . stem . ".exe"
    if FileExist(debugFromParent)
        return debugFromParent
    return sameDir
}

SimpleSttTempFile(prefix := "simple-stt") {
    dir := A_Temp . "\simple-stt-shell"
    DirCreate(dir)
    return dir . "\" . prefix . "-" . A_TickCount . "-" . Random(100000, 999999) . ".txt"
}

SimpleSttRandomToken() {
    bytes := Buffer(32, 0)
    ok := DllCall("advapi32\SystemFunction036", "Ptr", bytes.Ptr, "UInt", bytes.Size, "Int")
    token := ""
    if ok {
        Loop bytes.Size
            token .= Format("{:02x}", NumGet(bytes, A_Index - 1, "UChar"))
        return token
    }
    Loop 8
        token .= Format("{:08x}", Random(0, 0x7fffffff))
    return token
}

SimpleSttBool(value) {
    return value = true || value = 1 || StrLower(value . "") = "true"
}

SimpleSttBoolText(value) {
    return SimpleSttBool(value) ? "true" : "false"
}

SimpleSttWriteStdOut(text := "") {
    FileAppend(text, "*", "UTF-8")
}

SimpleSttWriteStdErr(text := "") {
    FileAppend(text, "**", "UTF-8")
}

SimpleSttConsoleLine(text := "") {
    SimpleSttWriteStdOut(text . "`n")
}

SimpleSttConsoleError(text) {
    SimpleSttWriteStdErr(text . "`n")
}
