from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

def replace(path: str, old: str, new: str) -> None:
    target = ROOT / path
    text = target.read_text(encoding="utf-8")
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{path}: expected one match, found {count}")
    target.write_text(text.replace(old, new), encoding="utf-8")

replace(
    "src/capture/ipc_server.rs",
    '''    let command: ClientMessage = read_json_line(&mut reader).context("reading command")?;
    let (request_id, command) = match command {
        ClientMessage::Command {
            request_id,
            command,
        } => (request_id, command),
        _ => anyhow::bail!("expected a command after handshake"),
    };
    let (reply_tx, reply_rx) = bounded(1);
    control_tx
        .send(ControlRequest {
            command,
            reply: reply_tx,
        })
        .context("forwarding command to capture loop")?;
''',
    '''    loop {
        let command: ClientMessage = match read_json_line(&mut reader) {
            Ok(command) => command,
            Err(error) if error.to_string().contains("unexpected EOF") => return Ok(()),
            Err(error) => return Err(error).context("reading command"),
        };
        let (request_id, command) = match command {
            ClientMessage::Command { request_id, command } => (request_id, command),
            _ => anyhow::bail!("expected a command after handshake"),
        };
        let (reply_tx, reply_rx) = bounded(1);
        control_tx
            .send(ControlRequest { command, reply: reply_tx })
            .context("forwarding command to capture loop")?;
''')