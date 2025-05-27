use std::{collections::HashMap, net::SocketAddr};

use regex::Regex;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{TcpStream, tcp::OwnedWriteHalf},
    sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel},
};
use tracing::{error, info, instrument, trace};

use crate::ServerImpl;

#[derive(Clone)]
pub struct ChatImpl {
    tx: UnboundedSender<Packet>,
}

#[instrument("chat-server", skip_all)]
async fn start_server(mut rx: UnboundedReceiver<Packet>) {
    info!("Started the chat server");
    let mut users = HashMap::new();
    while let Some(message) = rx.recv().await {
        match message {
            Packet::NewConnection(mut stream, addr) => {
                info!(ip = addr.to_string(), "Received new connection");
                let _ = stream.write_all(b"Please enter your username...\n").await;
                users.insert(
                    addr,
                    User {
                        stream,
                        username: String::new(),
                    },
                );
            }
            Packet::NewMessage(addr, message) => {
                let mut disconnected = Vec::new();

                let (sender_username, just_joined) = {
                    let needs_username = users
                        .get(&addr)
                        .map(|u| u.username.is_empty())
                        .unwrap_or(false);

                    if needs_username {
                        let trimmed = message.trim();
                        let is_invalid = !is_valid_username(trimmed)
                            || users.values().any(|u| u.username == trimmed);

                        if is_invalid {
                            let sender = users.get_mut(&addr).unwrap();
                            let _ = sender.stream.write_all(b"Invalid username...\n").await;
                            let _ = sender.stream.shutdown().await;
                            continue;
                        }

                        let usernames = users
                            .values()
                            .filter(|u| !u.username.is_empty())
                            .map(|u| u.username.as_str())
                            .collect::<Vec<_>>()
                            .join(", ");

                        let sender = users.get_mut(&addr).unwrap();

                        if !usernames.is_empty() {
                            let _ = sender
                                .stream
                                .write_all(
                                    format!("* The room contains: {}\n", usernames).as_bytes(),
                                )
                                .await;
                        } else {
                            let _ = sender
                                .stream
                                .write_all(b"* The room is currently empty\n")
                                .await;
                        }

                        trace!(
                            ip = addr.to_string(),
                            username = trimmed.to_string(),
                            "User set their username"
                        );
                        sender.username = trimmed.to_string();
                        (sender.username.as_str(), true)
                    } else {
                        let sender = users.get_mut(&addr).unwrap();
                        (sender.username.as_str(), false)
                    }
                };

                let message = if just_joined {
                    format!("* {} has entered the room\n", sender_username)
                } else {
                    trace!(
                        ip = addr.to_string(),
                        message = message.clone(),
                        "User sent new message"
                    );
                    format!("[{}] {}\n", sender_username, message)
                };

                for (target_addr, u) in users.iter_mut() {
                    if target_addr != &addr && !u.username.is_empty() {
                        if let Err(e) = u.stream.write_all(message.as_bytes()).await {
                            error!(
                                ip = addr.to_string(),
                                err = e.to_string(),
                                "Could not write to stream"
                            );
                            disconnected.push(*target_addr);
                        }
                    }
                }

                for addr in disconnected {
                    users.remove(&addr);
                }
            }
            Packet::RemoveConnection(addr) => {
                info!(ip = addr.to_string(), "Client disconnected");
                let user = users.remove(&addr).unwrap();
                if !user.username.is_empty() {
                    for (_, u) in users.iter_mut().filter(|(_, u)| !u.username.is_empty()) {
                        let _ = u
                            .stream
                            .write_all(
                                format!("* {} has left the room\n", user.username).as_bytes(),
                            )
                            .await;
                    }
                }
            }
        }
    }
}

impl ServerImpl for ChatImpl {
    fn new() -> Self {
        let (tx, rx) = unbounded_channel::<Packet>();

        tokio::spawn(async move { start_server(rx).await });
        ChatImpl { tx }
    }

    #[instrument("handle_client", skip_all)]
    async fn handle_client(&self, stream: TcpStream, addr: SocketAddr) {
        let (stream, write_stream) = stream.into_split();

        let _ = self.tx.send(Packet::NewConnection(write_stream, addr));

        let _guard = ConnectionGuard {
            addr,
            tx: self.tx.clone(),
        };

        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line).await {
                Ok(n) => {
                    if n == 0 {
                        info!(ip = addr.to_string(), "Connection closed");
                        break;
                    }
                }
                Err(e) => {
                    error!(
                        ip = addr.to_string(),
                        err = e.to_string(),
                        "Could not read from stream"
                    );
                    break;
                }
            };

            // Remove the \n or \r from end
            line.truncate(line.trim_end().len());

            if let Err(e) = self.tx.send(Packet::NewMessage(addr, line.clone())) {
                error!(
                    ip = addr.to_string(),
                    err = e.to_string(),
                    "Could not write to channel"
                );
                break;
            }
        }
    }
}

enum Packet {
    NewConnection(OwnedWriteHalf, SocketAddr),
    NewMessage(SocketAddr, String),
    RemoveConnection(SocketAddr),
}

struct User {
    stream: OwnedWriteHalf,
    username: String,
}

struct ConnectionGuard {
    addr: SocketAddr,
    tx: UnboundedSender<Packet>,
}

impl Drop for ConnectionGuard {
    fn drop(&mut self) {
        let _ = self.tx.send(Packet::RemoveConnection(self.addr));
    }
}

fn is_valid_username(username: &str) -> bool {
    // Compile the regex only once
    lazy_static::lazy_static! {
        static ref USERNAME_RE: Regex = Regex::new(r"^[a-zA-Z0-9]*$").unwrap();
    }

    USERNAME_RE.is_match(username)
}
