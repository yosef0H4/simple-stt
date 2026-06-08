class HotkeySpec {
    static Parse(label) {
        parts := StrSplit(label, "+")
        if parts.Length < 2
            throw Error("A dictation hotkey needs at least one modifier or CapsLock plus one final key.")
        finalKey := Trim(parts.Pop())
        if finalKey = ""
            throw Error("The final hotkey key is empty.")
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
        if usesCapsLock {
            down := "CapsLock & " . finalKey
            up := down . " up"
        } else {
            symbols := ""
            for modifier in ["LCtrl", "RCtrl", "Ctrl", "LAlt", "RAlt", "AltGr", "Alt", "LShift", "RShift", "Shift", "LWin", "RWin", "Win"] {
                if required.Has(modifier)
                    symbols .= this.ModifierSymbol(modifier)
            }
            down := "$*" . symbols . finalKey
            up := down . " up"
        }
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

    static RequiredModifiersDown(spec) {
        for modifier, _ in spec["required"] {
            if !this.ModifierIsDown(modifier)
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

class HotkeyManager {
    __New(onDown, onUp, logger) {
        this.onDown := onDown
        this.onUp := onUp
        this.logger := logger
        this.spec := ""
        this.enabled := false
        this.recording := false
        this.capsPending := false
        this.capsConsumed := false
        this.capslockBehavior := "preserve_tap"
        this.downCallback := ObjBindMethod(this, "HandleDown")
        this.upCallback := ObjBindMethod(this, "HandleUp")
        this.capsDownCallback := ObjBindMethod(this, "HandleCapsDown")
        this.capsUpCallback := ObjBindMethod(this, "HandleCapsUp")
    }

    Configure(label, enabled, capslockBehavior) {
        this.DisableBindings()
        this.spec := HotkeySpec.Parse(label)
        this.capslockBehavior := capslockBehavior
        this.enabled := enabled
        if enabled
            this.EnableBindings()
        this.logger.Write("info", "hotkey configured label=" . this.spec["label"] . " enabled=" . UvoxBoolText(enabled))
    }

    EnableBindings() {
        Hotkey(this.spec["down"], this.downCallback, "On")
        Hotkey(this.spec["up"], this.upCallback, "On")
        if this.spec["uses_capslock"] {
            Hotkey("*CapsLock", this.capsDownCallback, "On")
            Hotkey("*CapsLock up", this.capsUpCallback, "On")
        }
    }

    DisableBindings() {
        if this.recording {
            this.recording := false
            try this.onUp.Call()
        }
        if !IsObject(this.spec)
            return
        for binding in [this.spec["down"], this.spec["up"]]
            try Hotkey(binding, "Off")
        if this.spec["uses_capslock"] {
            try Hotkey("*CapsLock", "Off")
            try Hotkey("*CapsLock up", "Off")
        }
        this.recording := false
    }

    SetEnabled(enabled) {
        if this.enabled = enabled
            return
        this.DisableBindings()
        this.enabled := enabled
        if enabled
            this.EnableBindings()
    }

    HandleDown(*) {
        if !this.enabled || this.recording || !HotkeySpec.RequiredModifiersDown(this.spec)
            return
        this.recording := true
        if this.spec["uses_capslock"]
            this.capsConsumed := true
        this.onDown.Call()
    }

    HandleUp(*) {
        if !this.recording
            return
        this.recording := false
        this.onUp.Call()
    }

    HandleCapsDown(*) {
        this.capsPending := true
        this.capsConsumed := false
    }

    HandleCapsUp(*) {
        if !this.capsPending
            return
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

class HotkeyRecorder {
    __New(logger) {
        this.logger := logger
        this.input := ""
        this.onComplete := ""
    }

    Start(onComplete) {
        if IsObject(this.input)
            try this.input.Stop()
        this.onComplete := onComplete
        input := InputHook("L1")
        input.KeyOpt("{All}", "E")
        input.KeyOpt("{LCtrl}{RCtrl}{LAlt}{RAlt}{LShift}{RShift}{LWin}{RWin}{CapsLock}", "-E")
        input.OnEnd := ObjBindMethod(this, "Finish")
        this.input := input
        input.Start()
    }

    Finish(input) {
        key := input.EndKey
        if key = ""
            return
        parts := Array()
        if GetKeyState("CapsLock", "P")
            parts.Push("CapsLock")
        if GetKeyState("LCtrl", "P")
            parts.Push("LCtrl")
        if GetKeyState("RCtrl", "P")
            parts.Push("RCtrl")
        if GetKeyState("LAlt", "P")
            parts.Push("LAlt")
        if GetKeyState("RAlt", "P")
            parts.Push("RAlt")
        if GetKeyState("LShift", "P")
            parts.Push("LShift")
        if GetKeyState("RShift", "P")
            parts.Push("RShift")
        if GetKeyState("LWin", "P")
            parts.Push("LWin")
        if GetKeyState("RWin", "P")
            parts.Push("RWin")
        parts.Push(StrUpper(key))
        label := ""
        for index, part in parts
            label .= (index > 1 ? "+" : "") . part
        try HotkeySpec.Parse(label)
        catch Error as err {
            this.logger.Write("warning", "recorded hotkey rejected: " . err.Message)
            return
        }
        if IsObject(this.onComplete)
            this.onComplete.Call(label)
    }
}
