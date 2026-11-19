#Requires AutoHotkey v2.0
#SingleInstance Force
#Include ..\lib\Locale.ahk
#Include ..\lib\Tray.ahk

class TrayFixture {
    config := this
    Bool(_, fallback) => fallback
    __Call(name, params) => 0
}

TrayController.IconDirectory := A_ScriptDir . "\..\icons"
UiSetLanguage("en")
tray := TrayController(TrayFixture())
if !InStr(A_IconFile, "simple-stt.ico")
    throw Error("Simple STT tray icon was not set",, A_IconFile)

for language in ["en", "ar"] {
    UiSetLanguage(language)
    tray.Rebuild()
    handle := A_TrayMenu.Handle
    count := DllCall("GetMenuItemCount", "Ptr", handle)
    bitmapCount := 0
    readAlongFound := false
    Loop count {
        position := A_Index - 1
        text := Buffer(512, 0)
        length := DllCall("GetMenuStringW", "Ptr", handle, "UInt", position, "Ptr", text, "Int", 256, "UInt", 0x400)
        if !length
            continue
        label := StrGet(text, length, "UTF-16")
        info := Buffer(A_PtrSize = 8 ? 80 : 48, 0)
        NumPut("UInt", info.Size, "UInt", 0x80, info)
        if !DllCall("GetMenuItemInfoW", "Ptr", handle, "UInt", position, "Int", true, "Ptr", info)
            throw OSError()
        if !NumGet(info, info.Size - A_PtrSize, "Ptr")
            throw Error("Missing native menu icon",, label)
        bitmapCount += 1
        readAlongFound := readAlongFound || label = (language = "ar" ? "بدء القراءة مع التنقل" : "Start read-along")
    }
    if !readAlongFound || bitmapCount < 10
        throw Error("Incomplete localized tray menu",, language)
}
FileAppend("PASS: Simple STT tray icon and localized native menu bitmaps`n", "*")
if A_Args.Length && A_Args[1] = "--show" {
    UiSetLanguage("en")
    tray.Rebuild()
    CoordMode("Menu", "Screen")
    A_TrayMenu.Show(320, 240)
}
ExitApp()
