use std::{collections::HashMap, net::SocketAddr, sync::Arc};

use log::{error, info, trace};
use regex::Regex;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream, tcp::OwnedWriteHalf},
    sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel},
};

struct Chat {
    tx: UnboundedSender<Packet>,
}

impl Chat {
    fn new() -> Self {
        let (tx, rx) = unbounded_channel::<Packet>();

        tokio::spawn(async move { start_server(rx).await });
        Self { tx }
    }

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
                        info!("Connection closed ip={addr}");
                        break;
                    }
                }
                Err(e) => {
                    error!("Could not read from stream: {e} ip={addr}");
                    break;
                }
            };

            // Remove the \n or \r from end
            line.truncate(line.trim_end().len());

            if let Err(e) = self.tx.send(Packet::NewMessage(addr, line.clone())) {
                error!("Could not write to channel: {e} ip={addr}");
                break;
            }
        }
    }
}

async fn start_server(mut rx: UnboundedReceiver<Packet>) {
    info!("Started the chat server");
    let mut users = HashMap::new();
    while let Some(message) = rx.recv().await {
        match message {
            Packet::NewConnection(mut stream, addr) => {
                info!("Received new connection ip={addr}");
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

                        trace!("User set their username ip={addr} username={trimmed}",);
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
                    trace!("User sent new message ip={addr} message={message}");
                    format!("[{}] {}\n", sender_username, message)
                };

                for (target_addr, u) in users.iter_mut() {
                    if target_addr != &addr && !u.username.is_empty() {
                        if let Err(e) = u.stream.write_all(message.as_bytes()).await {
                            error!("Could not write to stream: {e} ip={addr}");
                            disconnected.push(*target_addr);
                        }
                    }
                }

                for addr in disconnected {
                    users.remove(&addr);
                }
            }
            Packet::RemoveConnection(addr) => {
                info!("Client disconnected ip={addr}");
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

pub async fn run_chat() {
    let listener = TcpListener::bind("0.0.0.0:8080")
        .await
        .unwrap_or_else(|e| panic!("Could not bind listener: {e}"));

    info!("🚀 Server listening on :8080");

    let chat = Arc::new(Chat::new());

    loop {
        match listener.accept().await {
            Ok((stream, addr)) => {
                let chat = chat.clone();
                tokio::spawn(async move {
                    chat.handle_client(stream, addr).await;
                });
            }
            Err(e) => {
                error!("Could not accept connection: {e}");
            }
        }
    }
}
