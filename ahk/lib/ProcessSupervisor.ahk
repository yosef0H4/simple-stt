class ProcessSupervisor {
    __New(captureExe, ctlExe, config, logger, onRestart) {
        this.captureExe := captureExe
        this.ctlExe := ctlExe
        this.config := config
        this.logger := logger
        this.onRestart := onRestart
        this.stateFile := config.Get("service_state_path")
        this.configFile := config.Get("config_path")
        this.token := UvoxRandomToken()
        this.pid := 0
        this.expectedStop := false
        this.restartAfterStop := false
        this.shutdownDeadline := 0
        this.readyProbeInFlight := false
        this.ipc := ""
        this.startTimer := ObjBindMethod(this, "Start")
        this.monitorTimer := ObjBindMethod(this, "Monitor")
        this.readyTimer := ObjBindMethod(this, "ProbeReady")
        this.shutdownTimer := ObjBindMethod(this, "PollShutdown")
    }

    AttachIpc(ipc) {
        this.ipc := ipc
    }

    Start(*) {
        if this.pid && ProcessExist(this.pid)
            return
        this.expectedStop := false
        this.restartAfterStop := false
        this.readyProbeInFlight := false
        this.token := UvoxRandomToken()
        if IsObject(this.ipc)
            this.ipc.ResetServiceSession(this.token)
        if FileExist(this.stateFile)
            try FileDelete(this.stateFile)
        command := UvoxQuote(this.captureExe) . " --token " . UvoxQuote(this.token) . " --state-file " . UvoxQuote(this.stateFile) . " --config " . UvoxQuote(this.configFile)
        try Run(command, A_ScriptDir, "Hide", &pid)
        catch Error as err {
            this.pid := 0
            this.logger.Write("error", "capture-service launch failed: " . err.Message)
            TrayTip("Audio service failed to start — retrying…", "Uvox", 3)
            SetTimer(this.startTimer, -2000)
            return
        }
        this.pid := pid
        this.logger.Write("info", "capture-service start pid=" . pid)
        SetTimer(this.monitorTimer, 1000)
        SetTimer(this.readyTimer, 250)
        TrayTip("Audio service starting…", "Uvox", 1)
    }

    ProbeReady(*) {
        if !IsObject(this.ipc) || !this.pid || !ProcessExist(this.pid) || this.readyProbeInFlight
            return
        if !FileExist(this.stateFile)
            return
        this.readyProbeInFlight := true
        this.ipc.Ping(ObjBindMethod(this, "OnPing"))
    }

    OnPing(response) {
        this.readyProbeInFlight := false
        if !response["ok"]
            return
        this.ipc.ready := true
        SetTimer(this.ipc.pollEventsTimer, -1)
        SetTimer(this.readyTimer, 0)
        this.logger.Write("info", "capture-service ready pid=" . this.pid)
        TrayTip("Audio service ready", "Uvox", 1)
    }

    Monitor(*) {
        if !this.pid || ProcessExist(this.pid)
            return
        oldPid := this.pid
        this.pid := 0
        if IsObject(this.ipc)
            this.ipc.ready := false
        SetTimer(this.readyTimer, 0)
        this.readyProbeInFlight := false
        if this.expectedStop {
            this.CompleteAsyncShutdown(oldPid)
            return
        }
        this.logger.Write("error", "capture-service stopped unexpectedly pid=" . oldPid)
        TrayTip("Audio service stopped — restarting…", "Uvox", 2)
        if IsObject(this.onRestart)
            this.onRestart.Call()
        SetTimer(this.startTimer, -300)
    }

    Restart() {
        this.logger.Write("info", "capture-service manual restart requested")
        this.BeginAsyncShutdown(true)
    }

    BeginAsyncShutdown(restartAfterStop := false) {
        this.restartAfterStop := restartAfterStop
        this.readyProbeInFlight := false
        SetTimer(this.readyTimer, 0)
        if !this.pid {
            if restartAfterStop
                SetTimer(this.startTimer, -1)
            return
        }
        this.expectedStop := true
        if IsObject(this.ipc) {
            this.ipc.ready := false
            this.ipc.CallService("shutdown", ObjBindMethod(this, "OnShutdownRequested"), "shutdown")
        }
        this.shutdownDeadline := A_TickCount + 2200
        SetTimer(this.shutdownTimer, 100)
    }

    OnShutdownRequested(response) {
        if !response["ok"]
            this.logger.Write("warning", "graceful capture-service shutdown request failed: " . response["message"])
    }

    PollShutdown(*) {
        if !this.pid {
            this.CompleteAsyncShutdown(0)
            return
        }
        try closedPid := ProcessWaitClose(this.pid, 0.01)
        catch
            closedPid := 0
        if closedPid || !ProcessExist(this.pid) {
            oldPid := this.pid
            this.pid := 0
            this.CompleteAsyncShutdown(oldPid)
            return
        }
        if A_TickCount < this.shutdownDeadline
            return
        oldPid := this.pid
        this.logger.Write("warning", "capture-service graceful shutdown timeout; forcing pid=" . oldPid)
        try ProcessWaitClose(oldPid, 0.01)
        if ProcessExist(oldPid) {
            try ProcessClose(oldPid)
            try ProcessWaitClose(oldPid, 1)
        }
        this.pid := 0
        this.CompleteAsyncShutdown(oldPid)
    }

    CompleteAsyncShutdown(oldPid) {
        SetTimer(this.shutdownTimer, 0)
        SetTimer(this.monitorTimer, 0)
        if oldPid
            this.logger.Write("info", "capture-service stopped pid=" . oldPid)
        shouldRestart := this.restartAfterStop
        this.restartAfterStop := false
        this.expectedStop := false
        if shouldRestart
            SetTimer(this.startTimer, -150)
    }

    Shutdown() {
        SetTimer(this.startTimer, 0)
        SetTimer(this.monitorTimer, 0)
        SetTimer(this.readyTimer, 0)
        SetTimer(this.shutdownTimer, 0)
        if !this.pid
            return
        this.expectedStop := true
        output := UvoxTempFile("shutdown")
        command := UvoxQuote(this.ctlExe) . " --state-file " . UvoxQuote(this.stateFile) . " --token " . UvoxQuote(this.token) . " --output " . UvoxQuote(output) . " shutdown"
        try RunWait(command, A_ScriptDir, "Hide")
        catch Error as err
            this.logger.Write("warning", "graceful capture-service shutdown request failed: " . err.Message)
        try FileDelete(output)
        try ProcessWaitClose(this.pid, 2)
        if ProcessExist(this.pid) {
            this.logger.Write("warning", "capture-service graceful shutdown timeout; forcing pid=" . this.pid)
            try ProcessClose(this.pid)
            try ProcessWaitClose(this.pid, 1)
        }
        this.logger.Write("info", "capture-service stopped pid=" . this.pid)
        this.pid := 0
        if IsObject(this.ipc)
            this.ipc.ready := false
    }
}
