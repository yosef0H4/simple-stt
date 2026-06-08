use crate::common::shell_protocol::{
    ClientMessage, ServerMessage, ShellCommand, ShellResponse, SHELL_PROTOCOL_VERSION,
};
use anyhow::{Context, Result};
use crossbeam_channel::{bounded, Sender};
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

#[derive(Debug)]
pub struct ControlRequest {
    pub command: ShellCommand,
    pub reply: Sender<ShellResponse>,
}

pub struct IpcServer {
    pub address: String,
    _join: thread::JoinHandle<()>,
}

pub fn spawn(token: String, control_tx: Sender<ControlRequest>) -> Result<IpcServer> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .context("binding local capture-service control listener")?;
    let address = listener.local_addr()?.to_string();
    let listen_address = address.clone();
    let join = thread::spawn(move || {
        tracing::info!(address = %listen_address, "shell IPC server listening on loopback only");
        for incoming in listener.incoming() {
            match incoming {
                Ok(stream) => {
                    let token = token.clone();
                    let control_tx = control_tx.clone();
                    thread::spawn(move || {
                        if let Err(error) = handle_client(stream, &token, &control_tx) {
                            tracing::warn!(%error, "shell IPC client failed");
                        }
                    });
                }
                Err(error) => tracing::warn!(%error, "shell IPC accept failed"),
            }
        }
    });
    Ok(IpcServer {
        address,
        _join: join,
    })
}

fn handle_client(
    mut stream: TcpStream,
    expected_token: &str,
    control_tx: &Sender<ControlRequest>,
) -> Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let hello: ClientMessage = read_json_line(&mut reader).context("reading handshake")?;
    match hello {
        ClientMessage::Hello { protocol, token }
            if protocol == SHELL_PROTOCOL_VERSION && token == expected_token =>
        {
            write_json_line(
                &mut stream,
                &ServerMessage::HelloAck {
                    protocol: SHELL_PROTOCOL_VERSION,
                    service_pid: std::process::id(),
                },
            )?;
            tracing::debug!("shell IPC handshake accepted");
        }
        ClientMessage::Hello { protocol, .. } if protocol != SHELL_PROTOCOL_VERSION => {
            write_json_line(
                &mut stream,
                &ServerMessage::Error {
                    code: "protocol_mismatch".into(),
                    message: format!("expected protocol {SHELL_PROTOCOL_VERSION}, got {protocol}"),
                },
            )?;
            anyhow::bail!("shell protocol mismatch");
        }
        _ => {
            write_json_line(
                &mut stream,
                &ServerMessage::Error {
                    code: "unauthorized".into(),
                    message: "invalid handshake token".into(),
                },
            )?;
            anyhow::bail!("shell IPC handshake rejected");
        }
    }
    let command: ClientMessage = read_json_line(&mut reader).context("reading command")?;
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
    let response = reply_rx
        .recv_timeout(Duration::from_secs(30))
        .context("waiting for capture-loop response")?;
    write_json_line(
        &mut stream,
        &ServerMessage::Response {
            request_id,
            response,
        },
    )?;
    Ok(())
}

fn read_json_line<T: serde::de::DeserializeOwned>(reader: &mut impl BufRead) -> Result<T> {
    let mut line = String::new();
    let count = reader.read_line(&mut line)?;
    anyhow::ensure!(count > 0, "unexpected EOF");
    anyhow::ensure!(line.len() <= 1024 * 1024, "IPC line exceeded 1 MiB");
    serde_json::from_str(line.trim_end()).context("parsing IPC JSON")
}
fn write_json_line<T: serde::Serialize>(writer: &mut impl Write, message: &T) -> Result<()> {
    serde_json::to_writer(&mut *writer, message)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossbeam_channel::unbounded;

    fn send_hello(address: &str, message: &ClientMessage) -> ServerMessage {
        let mut stream = TcpStream::connect(address).unwrap();
        write_json_line(&mut stream, message).unwrap();
        read_json_line(&mut BufReader::new(stream)).unwrap()
    }

    #[test]
    fn malformed_line_is_rejected() {
        assert!(
            read_json_line::<ClientMessage>(&mut BufReader::new("not-json\n".as_bytes())).is_err()
        );
    }

    #[test]
    fn protocol_mismatch_is_rejected() {
        let (tx, _rx) = unbounded();
        let server = spawn("token".into(), tx).unwrap();
        let response = send_hello(
            &server.address,
            &ClientMessage::Hello {
                protocol: SHELL_PROTOCOL_VERSION + 1,
                token: "token".into(),
            },
        );
        assert!(
            matches!(response, ServerMessage::Error { ref code, .. } if code == "protocol_mismatch")
        );
    }

    #[test]
    fn incorrect_token_is_rejected() {
        let (tx, _rx) = unbounded();
        let server = spawn("expected".into(), tx).unwrap();
        let response = send_hello(
            &server.address,
            &ClientMessage::Hello {
                protocol: SHELL_PROTOCOL_VERSION,
                token: "wrong".into(),
            },
        );
        assert!(
            matches!(response, ServerMessage::Error { ref code, .. } if code == "unauthorized")
        );
    }
}
