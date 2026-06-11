#Requires AutoHotkey v2.0
#SingleInstance Force
#Warn All, StdOut

#Include ..\lib\Utils.ahk
#Include ..\lib\Logging.ahk
#Include ..\lib\TabProtocol.ahk
#Include ..\lib\Config.ahk
#Include ..\lib\Hotkeys.ahk
#Include ..\lib\SettingsGui.ahk

class PreviewIpc {
    __New() {
        this.calls := Array()
    }

    CallService(arguments, callback := "", kind := "command") {
        this.calls.Push(arguments)
        response := Map("ok", true, "message", "preview", "values", Map())
        if InStr(arguments, "list-inputs") = 1 {
            response["values"]["input.1"] := "Default USB microphone"
            response["values"]["input.2"] := "Studio headset microphone"
        } else if InStr(arguments, "list-models") = 1 {
            response["values"]["recommended_model"] := "tdt_ctc-110m-f16.gguf"
            response["values"]["installed_model.1"] := "tdt_ctc-110m-f16.gguf|268|true|f16"
            response["values"]["catalog_model.1"] := "tdt_ctc-110m-q4_k.gguf|131|false|q4_k"
            response["values"]["catalog_model.2"] := "ctc-0.6b-f16.gguf|1374|false|f16"
        }
        if IsObject(callback)
            SetTimer(() => callback(response), -20)
        return 0
    }
}

class PreviewApp {
    __New() {
        this.testMode := true
        this.openLogCount := 0
        this.testModelCount := 0
        this.saveApplyCount := 0
        ctlExe := SimpleSttResolveExe("simple-stt-ctl")
        this.config := ConfigStore(ctlExe)
        this.logger := ShellLog(A_Temp . "\simple-stt-gui-preview.log")
        this.ipc := PreviewIpc()
    }

    OpenLatestLog(*) {
        this.openLogCount += 1
    }

    ApplySavedConfig(*) {
        this.saveApplyCount += 1
    }

    TestModel(*) {
        this.testModelCount += 1
    }
}

class GuiLoop {
    __New(settings, app, outputDir) {
        this.settings := settings
        this.app := app
        this.outputDir := outputDir
        this.lines := Array()
        this.failures := Array()
        this.pngToken := 0
    }

    Run() {
        DirCreate(this.outputDir)
        this.CleanOldScreenshots()
        this.Note("SimpleStt GUI loop started")
        this.ExerciseButtons()
        this.CaptureTabs("default", 900, 700)
        this.CaptureTabs("compact", 780, 650)
        this.CaptureTabs("wide", 1120, 760)
        this.settings.controls["tabs"].Choose(1)
        this.settings.gui.Show("w900 h700")
        this.settings.controls["save"].Focus()
        Sleep(250)
        this.Capture("final-general.png")
        this.WriteReport()
        this.StopGdiPlus()
        this.settings.gui.Destroy()
        ExitApp(this.failures.Length ? 1 : 0)
    }

    CleanOldScreenshots() {
        Loop Files this.outputDir . "\*.png"
            FileDelete(A_LoopFileFullPath)
        report := this.outputDir . "\report.txt"
        if FileExist(report)
            FileDelete(report)
    }

    ExerciseButtons() {
        tests := [
            [1, "record_chord", "Preview: shortcut recorder opened safely"],
            [2, "list_microphones", "Microphone list refreshed"],
            [2, "refresh_models", "Installed and downloadable model lists refreshed"],
            [2, "download_model", "Model download queued:"],
            [2, "test_model", "Model test queued"],
            [4, "browse_models", "Preview: folder picker opened safely"],
            [4, "open_models", "Preview: model folder opened safely"],
            [0, "save", "Settings saved"],
            [0, "reload", "Settings reloaded"]
        ]
        for pair in tests {
            this.ClickButton(pair[2], pair[1])
            Sleep(180)
            status := this.settings.controls["status"].Text
            if !InStr(status, pair[3])
                this.Fail(pair[2] . " status mismatch: " . status)
            else
                this.Note("PASS button " . pair[2] . " => " . status)
        }
        this.ClickButton("open_log")
        Sleep(80)
        this.Assert(this.app.openLogCount = 1, "open_log invoked app callback")
        this.Assert(this.app.testModelCount = 1, "test_model invoked app callback")
        this.Assert(this.app.saveApplyCount = 1, "save invoked app callback")
        this.Assert(this.HasIpcCall("list-inputs"), "list_microphones invoked IPC")
        this.Assert(this.HasIpcCall("refresh-models"), "refresh_models invoked IPC")
        this.Assert(this.HasIpcCall("download-model --filename"), "download_model invoked IPC")
    }

    ClickButton(key, tabIndex := 0) {
        if tabIndex {
            this.settings.controls["tabs"].Choose(tabIndex)
            Sleep(100)
        }
        control := this.settings.controls[key]
        DllCall("user32\SendMessage", "ptr", control.Hwnd, "uint", 0x00F5, "ptr", 0, "ptr", 0)
        Sleep(75)
    }

    HasIpcCall(prefix) {
        for call in this.app.ipc.calls
            if InStr(call, prefix) = 1
                return true
        return false
    }

    CaptureTabs(label, width, height) {
        this.settings.gui.Show("w" . width . " h" . height)
        Sleep(250)
        tabs := ["general", "audio-models", "output", "advanced"]
        Loop tabs.Length {
            this.settings.controls["tabs"].Choose(A_Index)
            Sleep(180)
            filename := label . "-" . tabs[A_Index] . ".png"
            this.Capture(filename)
        }
    }

    Capture(filename) {
        path := this.outputDir . "\" . filename
        this.CaptureWindowPng(this.settings.gui.Hwnd, path)
        this.Assert(FileExist(path), "screenshot saved " . filename)
    }

    CaptureWindowPng(hwnd, path) {
        rect := Buffer(16, 0)
        if !DllCall("user32\GetWindowRect", "ptr", hwnd, "ptr", rect)
            throw Error("GetWindowRect failed")
        left := NumGet(rect, 0, "int")
        top := NumGet(rect, 4, "int")
        width := NumGet(rect, 8, "int") - left
        height := NumGet(rect, 12, "int") - top
        screenDc := DllCall("user32\GetDC", "ptr", 0, "ptr")
        memoryDc := DllCall("gdi32\CreateCompatibleDC", "ptr", screenDc, "ptr")
        bitmap := DllCall("gdi32\CreateCompatibleBitmap", "ptr", screenDc, "int", width, "int", height, "ptr")
        previous := DllCall("gdi32\SelectObject", "ptr", memoryDc, "ptr", bitmap, "ptr")
        ok := DllCall("user32\PrintWindow", "ptr", hwnd, "ptr", memoryDc, "uint", 0)
        if !ok
            DllCall("gdi32\BitBlt", "ptr", memoryDc, "int", 0, "int", 0, "int", width, "int", height, "ptr", screenDc, "int", left, "int", top, "uint", 0x00CC0020)
        DllCall("gdi32\SelectObject", "ptr", memoryDc, "ptr", previous)
        this.SaveBitmapPng(bitmap, path)
        DllCall("gdi32\DeleteObject", "ptr", bitmap)
        DllCall("gdi32\DeleteDC", "ptr", memoryDc)
        DllCall("user32\ReleaseDC", "ptr", 0, "ptr", screenDc)
    }

    SaveBitmapPng(bitmap, path) {
        if !this.pngToken
            this.StartGdiPlus()
        image := 0
        status := DllCall("gdiplus\GdipCreateBitmapFromHBITMAP", "ptr", bitmap, "ptr", 0, "ptr*", &image)
        if status != 0
            throw Error("GdipCreateBitmapFromHBITMAP failed status=" . status)
        encoder := Buffer(16, 0)
        NumPut("uint", 0x557CF406, encoder, 0)
        NumPut("ushort", 0x1A04, encoder, 4)
        NumPut("ushort", 0x11D3, encoder, 6)
        NumPut("uchar", 0x9A, encoder, 8)
        NumPut("uchar", 0x73, encoder, 9)
        NumPut("uchar", 0x00, encoder, 10)
        NumPut("uchar", 0x00, encoder, 11)
        NumPut("uchar", 0xF8, encoder, 12)
        NumPut("uchar", 0x1E, encoder, 13)
        NumPut("uchar", 0xF3, encoder, 14)
        NumPut("uchar", 0x2E, encoder, 15)
        status := DllCall("gdiplus\GdipSaveImageToFile", "ptr", image, "wstr", path, "ptr", encoder, "ptr", 0)
        DllCall("gdiplus\GdipDisposeImage", "ptr", image)
        if status != 0
            throw Error("GdipSaveImageToFile failed status=" . status)
    }

    StartGdiPlus() {
        input := Buffer(A_PtrSize = 8 ? 24 : 16, 0)
        NumPut("uint", 1, input, 0)
        token := 0
        status := DllCall("gdiplus\GdiplusStartup", "ptr*", &token, "ptr", input, "ptr", 0)
        if status != 0
            throw Error("GdiplusStartup failed status=" . status)
        this.pngToken := token
    }

    StopGdiPlus() {
        if !this.pngToken
            return
        token := this.pngToken
        this.pngToken := 0
        try DllCall("gdiplus\GdiplusShutdown", "ptr", token)
    }

    Assert(condition, message) {
        if condition
            this.Note("PASS " . message)
        else
            this.Fail(message)
    }

    Note(message) {
        this.lines.Push(message)
    }

    Fail(message) {
        this.failures.Push(message)
        this.lines.Push("FAIL " . message)
    }

    WriteReport() {
        this.lines.Push("")
        this.lines.Push(this.failures.Length ? "RESULT: FAIL" : "RESULT: PASS")
        this.lines.Push("screenshots: 13")
        FileAppend(this.JoinLines(this.lines), this.outputDir . "\report.txt", "UTF-8")
    }

    JoinLines(lines) {
        text := ""
        for line in lines
            text .= line . "`r`n"
        return text
    }
}

tempDir := A_Temp . "\simple-stt-gui-preview"
DirCreate(tempDir)
configPath := tempDir . "\config.json"
if FileExist(configPath)
    FileDelete(configPath)
EnvSet("SIMPLE_STT_CONFIG", configPath)

OnError(PreviewOnError)
app := PreviewApp()
settings := SettingsGui(app)
settings.Open()
Sleep(300)
outputDir := A_ScriptDir . "\..\..\artifacts\gui-loop"
previewRunner := GuiLoop(settings, app, outputDir)
SetTimer(ObjBindMethod(previewRunner, "Run"), -200)
Persistent

PreviewOnError(error, mode) {
    SimpleSttConsoleError("UNHANDLED ERROR: " . error.Message . "`n" . error.Stack)
    ExitApp(1)
    return true
}
