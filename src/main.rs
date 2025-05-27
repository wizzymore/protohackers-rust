use std::{collections::HashMap, net::SocketAddr};

use regex::Regex;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream, tcp::OwnedWriteHalf},
    sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel},
};

enum Message {
    NewConnection(OwnedWriteHalf, SocketAddr),
    NewMessage(SocketAddr, String),
    RemoveConnection(SocketAddr),
}

#[tokio::main]
async fn main() {
    let listener = TcpListener::bind("localhost:8080")
        .await
        .unwrap_or_else(|e| panic!("Could not bind listener: {e}"));

    let (tx, rx) = unbounded_channel::<Message>();

    tokio::spawn(async move { start_server(rx).await });

    loop {
        match listener.accept().await {
            Ok((stream, addr)) => {
                let tx = tx.clone();
                tokio::spawn(async move { handle_client(stream, addr, tx).await });
            }
            Err(e) => {
                eprintln!("Could not accept connection: {e}");
            }
        }
    }
}

fn is_valid_username(username: &str) -> bool {
    // Compile the regex only once
    lazy_static::lazy_static! {
        static ref USERNAME_RE: Regex = Regex::new(r"^[a-zA-Z0-9]*$").unwrap();
    }

    USERNAME_RE.is_match(username)
}

struct User {
    stream: OwnedWriteHalf,
    username: String,
}

async fn start_server(mut rx: UnboundedReceiver<Message>) {
    let mut users = HashMap::new();
    while let Some(message) = rx.recv().await {
        match message {
            Message::NewConnection(mut stream, addr) => {
                let _ = stream.write_all(b"Please enter your username...\n").await;
                users.insert(
                    addr,
                    User {
                        stream,
                        username: String::new(),
                    },
                );
            }
            Message::NewMessage(addr, message) => {
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

                        if usernames.len() > 0 {
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
                    format!("[{}] {}\n", sender_username, message)
                };

                for (target_addr, u) in users.iter_mut() {
                    if target_addr != &addr {
                        if let Err(e) = u.stream.write_all(message.as_bytes()).await {
                            eprintln!("Could not write to stream {addr}: {e}");
                            disconnected.push(*target_addr);
                        }
                    }
                }

                for addr in disconnected {
                    users.remove(&addr);
                }
            }
            Message::RemoveConnection(addr) => {
                println!("Client disconnected: {addr}");
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

struct ConnectionGuard {
    addr: SocketAddr,
    tx: UnboundedSender<Message>,
}

impl Drop for ConnectionGuard {
    fn drop(&mut self) {
        let _ = self.tx.send(Message::RemoveConnection(self.addr));
    }
}

async fn handle_client(stream: TcpStream, addr: SocketAddr, tx: UnboundedSender<Message>) {
    let (stream, write_stream) = stream.into_split();
    println!("Received new connection from: {addr}");

    let _ = tx.send(Message::NewConnection(write_stream, addr));

    let _guard = ConnectionGuard {
        addr,
        tx: tx.clone(),
    };

    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line).await {
            Ok(n) => {
                if n == 0 {
                    println!("Connection closed: {addr}");
                    break;
                }
            }
            Err(e) => {
                eprintln!("Could not read from stream: {e}");
                break;
            }
        };

        // Remove the \n or \r from end
        line.truncate(line.trim_end().len());

        if let Err(e) = tx.send(Message::NewMessage(addr, line.clone())) {
            eprintln!("Could not write to channel {e}");
            break;
        }
    }
}
