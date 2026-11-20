class HotkeySpec {
    static IsNone(label) {
        value := StrLower(Trim(label))
        return value = "none" || value = "disabled" || value = "off"
    }

    static Parse(label) {
        parts := StrSplit(label, "+")
        if parts.Length < 2
            throw Error("A dictation hotkey needs at least one modifier or CapsLock plus one final key.")
        finalKey := Trim(parts.Pop())
        if finalKey = ""
            throw Error("The final hotkey key is empty.")
        if this.NormalizeModifier(finalKey) != ""
            throw Error("An app shortcut requires a non-modifier key.")
        required := Map()
        for part in parts {
            normalized := this.NormalizeModifier(Trim(part))
            if normalized = ""
                throw Error("Unsupported hotkey modifier: " . part)
            required[normalized] := true
        }
        displayParts := Array()
        for modifier in ["LCtrl", "RCtrl", "Ctrl", "LAlt", "RAlt", "AltGr", "Alt", "LShift", "RShift", "Shift", "LWin", "RWin", "Win", "CapsLock"] {
            if required.Has(modifier)
                displayParts.Push(modifier)
        }
        displayParts.Push(StrUpper(finalKey))
        display := ""
        for index, part in displayParts
            display .= (index > 1 ? "+" : "") . part

        usesCapsLock := required.Has("CapsLock")
        symbols := ""
        for modifier in ["LCtrl", "RCtrl", "Ctrl", "LAlt", "RAlt", "AltGr", "Alt", "LShift", "RShift", "Shift", "LWin", "RWin", "Win"] {
            if required.Has(modifier)
                symbols .= this.ModifierSymbol(modifier)
        }
        down := "$*" . symbols . finalKey
        up := down . " up"
        return Map("label", display, "down", down, "up", up, "required", required, "uses_capslock", usesCapsLock)
    }

    static NormalizeModifier(value) {
        lower := StrLower(value)
        switch lower {
            case "ctrl", "control": return "Ctrl"
            case "lctrl", "leftctrl", "leftcontrol": return "LCtrl"
            case "rctrl", "rightctrl", "rightcontrol": return "RCtrl"
            case "alt": return "Alt"
            case "lalt", "leftalt": return "LAlt"
            case "ralt", "rightalt": return "RAlt"
            case "altgr": return "AltGr"
            case "shift": return "Shift"
            case "lshift", "leftshift": return "LShift"
            case "rshift", "rightshift": return "RShift"
            case "win", "windows": return "Win"
            case "lwin", "leftwin": return "LWin"
            case "rwin", "rightwin": return "RWin"
            case "capslock", "caps": return "CapsLock"
        }
        return ""
    }

    static ModifierSymbol(modifier) {
        switch modifier {
            case "Ctrl": return "^"
            case "LCtrl": return "<^"
            case "RCtrl": return ">^"
            case "Alt": return "!"
            case "LAlt": return "<!"
            case "RAlt": return ">!"
            case "AltGr": return "<^>!"
            case "Shift": return "+"
            case "LShift": return "<+"
            case "RShift": return ">+"
            case "Win": return "#"
            case "LWin": return "<#"
            case "RWin": return ">#"
        }
        return ""
    }

    static RequiredModifiersDown(spec, capsDown) {
        for modifier, _ in spec["required"] {
            if !(modifier = "CapsLock" ? capsDown : this.ModifierIsDown(modifier))
                return false
        }
        return true
    }

    static ModifierIsDown(modifier) {
        switch modifier {
            case "Ctrl": return GetKeyState("LCtrl", "P") || GetKeyState("RCtrl", "P")
            case "LCtrl": return GetKeyState("LCtrl", "P")
            case "RCtrl": return GetKeyState("RCtrl", "P")
            case "Alt": return GetKeyState("LAlt", "P") || GetKeyState("RAlt", "P")
            case "LAlt": return GetKeyState("LAlt", "P")
            case "RAlt", "AltGr": return GetKeyState("RAlt", "P")
            case "Shift": return GetKeyState("LShift", "P") || GetKeyState("RShift", "P")
            case "LShift": return GetKeyState("LShift", "P")
            case "RShift": return GetKeyState("RShift", "P")
            case "Win": return GetKeyState("LWin", "P") || GetKeyState("RWin", "P")
            case "LWin": return GetKeyState("LWin", "P")
            case "RWin": return GetKeyState("RWin", "P")
            case "CapsLock": return GetKeyState("CapsLock", "P")
        }
        return false
    }
}

class CapsLockTapController {
    __New(logger) {
        this.logger := logger
        this.count := 0
        this.installed := false
        this.capsPending := false
        this.capsConsumed := false
        this.capslockBehavior := "preserve_tap"
        this.capsDownCallback := ObjBindMethod(this, "HandleCapsDown")
        this.capsUpCallback := ObjBindMethod(this, "HandleCapsUp")
        this.hintTimer := ObjBindMethod(this, "ShowHint")
        this.hintWindow := ""
        this.hintWake := ""
        this.hintOnCaps := false
    }

    Register(capslockBehavior) {
        this.capslockBehavior := capslockBehavior
        this.count += 1
        if !this.installed {
            Hotkey("*CapsLock", this.capsDownCallback, "On")
            Hotkey("*CapsLock up", this.capsUpCallback, "On")
            this.installed := true
        }
    }

    Unregister() {
        if this.count > 0
            this.count -= 1
        if this.count = 0 && this.installed {
            try Hotkey("*CapsLock", "Off")
            try Hotkey("*CapsLock up", "Off")
            this.installed := false
            this.HideHint()
            this.capsPending := false
            this.capsConsumed := false
        }
    }

    ConfigureHint(label, capslockBehavior, instanceId) {
        enabled := !HotkeySpec.IsNone(label)
        bareCaps := StrLower(Trim(label)) = "capslock"
        this.HideHint()
        if enabled && !IsObject(this.hintWindow) {
            ; This hidden window owns the hold state, so it is cleared even if the shell exits unexpectedly.
            this.hintWindow := Gui("+ToolWindow", "SimpleSttShortcutHold-" . instanceId)
            this.hintWake := "Local\SimpleSttShortcutWake-" . instanceId
        } else if !enabled && IsObject(this.hintWindow) {
            this.hintWindow.Destroy()
            this.hintWindow := ""
        }
        if bareCaps && !this.hintOnCaps
            this.Register(capslockBehavior)
        else if !bareCaps && this.hintOnCaps
            this.Unregister()
        if enabled
            this.capslockBehavior := capslockBehavior
        this.hintOnCaps := bareCaps
    }

    ShowHint() {
        if IsObject(this.hintWindow) && (!this.hintOnCaps || this.capsPending) {
            DllCall("SetPropW", "ptr", this.hintWindow.Hwnd, "str", "SimpleSttShortcutHold", "ptr", 1)
            this.WakeOverlay()
        }
    }

    HideHint() {
        SetTimer(this.hintTimer, 0)
        if IsObject(this.hintWindow) {
            DllCall("RemovePropW", "ptr", this.hintWindow.Hwnd, "str", "SimpleSttShortcutHold", "ptr")
            this.WakeOverlay()
        }
    }

    WakeOverlay() {
        eventHandle := DllCall("OpenEventW", "uint", 2, "int", false, "str", this.hintWake, "ptr")
        if eventHandle {
            DllCall("SetEvent", "ptr", eventHandle)
            DllCall("CloseHandle", "ptr", eventHandle)
        }
    }

    Consume() {
        this.capsConsumed := true
    }

    HandleCapsDown(*) {
        if this.capsPending
            return
        this.capsPending := true
        this.capsConsumed := false
        if this.hintOnCaps
            SetTimer(this.hintTimer, -150)
    }

    HandleCapsUp(*) {
        if !this.capsPending
            return
        if this.hintOnCaps
            this.HideHint()
        if !this.capsConsumed {
            if this.capslockBehavior = "always_off"
                SetCapsLockState("Off")
            else
                SetCapsLockState(GetKeyState("CapsLock", "T") ? "Off" : "On")
        } else if this.capslockBehavior = "always_off" {
            SetCapsLockState("Off")
        }
        this.capsPending := false
        this.capsConsumed := false
    }
}
class HotkeyManager {
    __New(onDown, onUp, logger, capsController := "") {
        this.onDown := onDown
        this.onUp := onUp
        this.logger := logger
        this.capsController := IsObject(capsController) ? capsController : CapsLockTapController(logger)
        this.spec := ""
        this.enabled := false
        this.recording := false
        this.capslockBehavior := "preserve_tap"
        this.capsRegistered := false
        this.releaseStops := true
        this.downCallback := ObjBindMethod(this, "HandleDown")
        this.upCallback := ObjBindMethod(this, "HandleUp")
        this.capsContextCallback := ObjBindMethod(this, "CapsContext")
    }

    Configure(label, enabled, capslockBehavior, releaseStops := true) {
        this.DisableBindings()
        this.spec := HotkeySpec.IsNone(label) ? "" : HotkeySpec.Parse(label)
        this.capslockBehavior := capslockBehavior
        this.enabled := enabled && IsObject(this.spec)
        this.releaseStops := releaseStops
        if this.enabled
            this.EnableBindings()
        configuredLabel := IsObject(this.spec) ? this.spec["label"] : "None"
        this.logger.Write("info", "hotkey configured label=" . configuredLabel . " enabled=" . SimpleSttBoolText(this.enabled))
    }

    EnableBindings() {
        if !IsObject(this.spec)
            return
        if this.spec["uses_capslock"] {
            this.capsController.Register(this.capslockBehavior)
            this.capsRegistered := true
            HotIf(this.capsContextCallback)
        }
        try {
            Hotkey(this.spec["down"], this.downCallback, "On")
            Hotkey(this.spec["up"], this.upCallback, "On")
        } finally {
            HotIf()
        }
    }

    DisableBindings() {
        if this.recording {
            this.recording := false
            try this.onUp.Call()
        }
        if !IsObject(this.spec)
            return
        if this.spec["uses_capslock"]
            HotIf(this.capsContextCallback)
        try {
            for binding in [this.spec["down"], this.spec["up"]]
                try Hotkey(binding, "Off")
        } finally {
            HotIf()
        }
        if this.capsRegistered {
            this.capsController.Unregister()
            this.capsRegistered := false
        }
        this.recording := false
    }

    SetEnabled(enabled) {
        if this.enabled = enabled
            return
        this.DisableBindings()
        this.enabled := enabled && IsObject(this.spec)
        if this.enabled
            this.EnableBindings()
    }

    CapsContext(*) {
        return this.recording || this.capsController.capsPending
    }

    HandleDown(*) {
        if !this.enabled || this.recording || !HotkeySpec.RequiredModifiersDown(this.spec, this.capsController.capsPending)
            return
        this.recording := true
        if this.capsController.capsPending
            this.capsController.Consume()
        this.onDown.Call()
    }

    HandleUp(*) {
        if !this.recording
            return
        this.recording := false
        if this.releaseStops
            this.onUp.Call()
    }

}
