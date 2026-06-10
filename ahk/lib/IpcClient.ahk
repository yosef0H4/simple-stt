class IpcClient {
    __New(ctlExe, stateFile, token, eventHandler, logger) {
        this.ctlExe := ctlExe
        this.stateFile := stateFile
        this.token := token
        this.eventHandler := eventHandler
        this.logger := logger
        this.jobs := Map()
        this.latestSeq := 0
        this.pollInFlight := false
        this.ready := false
        this.useLongPoll := true
        this.pollJobsTimer := ObjBindMethod(this, "PollJobs")
        this.pollEventsTimer := ObjBindMethod(this, "PollEvents")
        this.pollResponseCallback := ObjBindMethod(this, "HandlePollResponse")
        this.retryTimer := ObjBindMethod(this, "RetryPing")
        ; Poll helper jobs only while work exists. A recurring idle timer used to
        ; wake the interpreter 20 times per second even when there was nothing
        ; to inspect. One-shot scheduling is both cheaper at idle and quicker
        ; for hotkey-triggered start/stop acknowledgements.
        SetTimer(this.pollEventsTimer, -1)
    }


    ResetServiceSession(token) {
        for pid, job in this.jobs {
            if ProcessExist(pid)
                try ProcessClose(pid)
            try FileDelete(job["path"])
        }
        this.jobs := Map()
        this.token := token
        this.latestSeq := 0
        this.pollInFlight := false
        this.ready := false
        this.useLongPoll := true
        SetTimer(this.retryTimer, 0)
    }

    Stop() {
        SetTimer(this.pollJobsTimer, 0)
        SetTimer(this.pollEventsTimer, 0)
        SetTimer(this.retryTimer, 0)
        for pid, job in this.jobs {
            if ProcessExist(pid)
                try ProcessClose(pid)
            try FileDelete(job["path"])
        }
        this.jobs := Map()
        this.pollInFlight := false
        this.ready := false
    }

    CallService(arguments, callback := "", kind := "command") {
        output := UvoxTempFile("ctl")
        command := UvoxQuote(this.ctlExe) . " --state-file " . UvoxQuote(this.stateFile) . " --token " . UvoxQuote(this.token) . " --output " . UvoxQuote(output) . " " . arguments
        try {
            Run(command, A_ScriptDir, "Hide", &pid)
            this.jobs[pid] := Map("path", output, "callback", callback, "kind", kind, "started", A_TickCount)
            SetTimer(this.pollJobsTimer, -1)
            return pid
        } catch Error as err {
            if kind = "events"
                this.pollInFlight := false
            this.logger.Write("error", "uvoxctl launch failed: " . err.Message)
            response := TabProtocol.ErrorResponse(err.Message)
            if IsObject(callback)
                callback.Call(response)
            return 0
        }
    }

    PollJobs(*) {
        finished := Array()
        for pid, job in this.jobs {
            responseReady := FileExist(job["path"])
            elapsed := A_TickCount - job["started"]
            if !responseReady && ProcessExist(pid) && elapsed < 35000
                continue
            if !responseReady && elapsed >= 35000 {
                if ProcessExist(pid)
                    try ProcessClose(pid)
                response := TabProtocol.ErrorResponse("uvoxctl helper timed out")
                this.logger.Write("error", "uvoxctl helper timeout pid=" . pid . " kind=" . job["kind"])
            } else {
                response := TabProtocol.ReadResponse(job["path"])
            }
            try FileDelete(job["path"])
            if job["kind"] = "events"
                this.pollInFlight := false
            if IsObject(job["callback"])
                try job["callback"].Call(response)
                catch Error as err
                    this.logger.Write("error", "IPC callback failed: " . err.Message)
            finished.Push(pid)
        }
        for pid in finished
            if this.jobs.Has(pid)
                this.jobs.Delete(pid)
        if this.jobs.Count {
            onlyEvents := true
            for _, job in this.jobs {
                if job["kind"] != "events" {
                    onlyEvents := false
                    break
                }
            }
            SetTimer(this.pollJobsTimer, onlyEvents ? -100 : -10)
        }
    }

    PollEvents(*) {
        if !this.ready || this.pollInFlight
            return
        this.pollInFlight := true
        arguments := "poll-events --after-seq " . this.latestSeq
        if this.useLongPoll
            arguments .= " --wait-ms 900"
        this.CallService(arguments, this.pollResponseCallback, "events")
    }

    HandlePollResponse(response) {
        if !response["ok"] {
            if this.useLongPoll {
                this.useLongPoll := false
                this.logger.Write("warning", "long event poll unavailable; falling back to short polling: " . response["message"])
                SetTimer(this.pollEventsTimer, -1)
                return
            }
            this.ready := false
            this.logger.Write("warning", "event poll failed: " . response["message"])
            SetTimer(this.retryTimer, -300)
            return
        }
        if response["values"].Has("latest_seq")
            this.latestSeq := response["values"]["latest_seq"] + 0
        for event in response["events"] {
            if event["seq"] > this.latestSeq
                this.latestSeq := event["seq"]
            try this.eventHandler.Call(event)
            catch Error as err
                this.logger.Write("error", "service event callback failed: " . err.Message)
        }
        if this.ready
            SetTimer(this.pollEventsTimer, -1)
    }

    Ping(callback := "") {
        finalCallback := callback
        if !IsObject(finalCallback)
            finalCallback := ObjBindMethod(this, "HandlePing")
        return this.CallService("ping", finalCallback, "ping")
    }

    HandlePing(response) {
        this.ready := response["ok"]
        if !this.ready {
            SetTimer(this.retryTimer, -500)
            return
        }
        SetTimer(this.pollEventsTimer, -1)
    }

    RetryPing(*) {
        if this.ready
            return
        this.Ping()
    }
}
