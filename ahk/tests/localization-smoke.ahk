#Requires AutoHotkey v2.0
#SingleInstance Force

#Include ..\lib\Locale.ahk

Fail(message) {
    FileAppend("FAIL: " . message . "`n", "*")
    ExitApp(1)
}

UiSetLanguage("en")
if UiText("menu.settings") != "Settings"
    Fail("English catalog lookup")
UiSetLanguage("ar")
if UiText("menu.settings") != "الإعدادات"
    Fail("Arabic catalog lookup")
if UiText("notice.delivery", Map("mode", "Ctrl+V {mode}")) != "🎙 الإدراج: Ctrl+V {mode}"
    Fail("interpolation must preserve inserted values")
UiSetLanguage("unsupported")
if UiText("menu.settings") != "Settings"
    Fail("unsupported language fallback")
if UiText("unknown.message") != "unknown.message"
    Fail("unknown key fallback")

FileAppend("PASS: localization smoke`n", "*")
ExitApp(0)
