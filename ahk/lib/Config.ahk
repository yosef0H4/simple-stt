class ConfigStore {
    __New(ctlExe) {
        this.ctlExe := ctlExe
        this.values := Map()
        this.LoadSync()
    }

    LoadSync() {
        output := SimpleSttTempFile("config-show")
        command := SimpleSttQuote(this.ctlExe) . " --output " . SimpleSttQuote(output) . " config-show"
        try RunWait(command, A_ScriptDir, "Hide")
        catch Error as err
            throw Error("Unable to run simple-stt-ctl config-show: " . err.Message)
        response := TabProtocol.ReadResponse(output)
        try FileDelete(output)
        if !response["ok"]
            throw Error("Unable to load SimpleStt config: " . response["message"])
        this.values := response["values"]
        return this
    }

    SaveSync() {
        input := SimpleSttTempFile("config-save-input")
        output := SimpleSttTempFile("config-save-output")
        keys := ["hotkey_enabled", "record_hotkey", "capslock_behavior", "audio_device_contains", "audio_gain", "typing_chunk_chars", "typing_interval_ms", "trailing_space", "text_delivery_mode", "remove_punctuation", "lowercase_output", "idle_worker_timeout_secs", "worker_shutdown_grace_ms", "start_with_windows", "log_level", "diagnostic_overlay", "log_transcripts", "inference_device", "parakeet_runtime_dir", "model_dir", "selected_model_filename"]
        text := ""
        for key in keys
            text .= TabProtocol.Escape(key) . "`t" . TabProtocol.Escape(this.Get(key, "")) . "`n"
        ; simple-stt-ctl expects tab-delimited UTF-8 without a BOM. AHK's "UTF-8"
        ; encoding writes a BOM which becomes part of the first config key.
        FileAppend(text, input, "UTF-8-RAW")
        command := SimpleSttQuote(this.ctlExe) . " --output " . SimpleSttQuote(output) . " config-save --input " . SimpleSttQuote(input)
        try RunWait(command, A_ScriptDir, "Hide")
        catch Error as err
            throw Error("Unable to run simple-stt-ctl config-save: " . err.Message)
        response := TabProtocol.ReadResponse(output)
        try FileDelete(input)
        try FileDelete(output)
        if !response["ok"]
            throw Error("Unable to save SimpleStt config: " . response["message"])
        this.values := response["values"]
        return this
    }

    Get(key, fallback := "") {
        return this.values.Has(key) ? this.values[key] : fallback
    }
    Set(key, value) {
        this.values[key] := value . ""
    }
    Bool(key, fallback := false) {
        return SimpleSttBool(this.Get(key, fallback ? "true" : "false"))
    }
    Int(key, fallback := 0) {
        value := this.Get(key, fallback)
        return value + 0
    }
}
