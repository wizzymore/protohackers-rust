use std::{char, collections::HashMap, net::SocketAddr, sync::Arc};

use lazy_static::lazy_static;
use log::{error, info};
use tokio::{
    net::UdpSocket,
    sync::{
        RwLock,
        mpsc::{UnboundedReceiver, unbounded_channel},
    },
};

const VERSION: &str = "Ken's Key-Value Store 1.0\n";

lazy_static! {
    static ref DATA: RwLock<HashMap<String, String>> = RwLock::new(HashMap::new());
}

enum Message {
    Insert(SocketAddr, String, String),
    Retrieve(SocketAddr, String),
}

async fn run_server(socket: Arc<UdpSocket>, mut rx: UnboundedReceiver<Message>) {
    loop {
        match rx.recv().await {
            Some(message) => {
                match message {
                    Message::Insert(addr, key, value) => {
                        info!("Client {addr} sent a insert request for `{key}` of `{value}`");
                        if key == "version" {
                            continue;
                        }
                        let mut data = DATA.write().await;
                        data.insert(key, value);
                    }
                    Message::Retrieve(addr, key) => {
                        info!("Client {addr} sent a get request for `{key}`");
                        match key.as_str() {
                            "version" => {
                                if socket.send_to(VERSION.as_bytes(), addr).await.is_err() {
                                    error!("Failed to reply to {addr} about key `{key}`");
                                }
                            }
                            key => {
                                let data = DATA.read().await;
                                let Some(value) = data.get(key) else {
                                    info!("Client {addr} requested inexistent key `{key}`");
                                    continue;
                                };

                                let mut reply = String::with_capacity(key.len() + value.len() + 2); // `=` + `\n`
                                reply.push_str(key);
                                reply.push('=');
                                reply.push_str(value);
                                reply.push('\n');

                                if socket.send_to(reply.as_bytes(), addr).await.is_err() {
                                    error!("Failed to reply to {addr} about key `{key}`");
                                }
                            }
                        };
                    }
                };
            }
            None => {
                break;
            }
        }
    }
}

pub async fn run_unusual() {
    let socket = Arc::new(UdpSocket::bind("0.0.0.0:8080").await.unwrap());

    info!("🚀 Server listening on :8080");

    let (tx, rx) = unbounded_channel();

    tokio::spawn(run_server(socket.clone(), rx));

    let mut buf = [0u8; 1000];
    loop {
        match socket.recv_from(&mut buf).await {
            Ok((n, addr)) => {
                info!("Received {n} bytes from {addr}");

                let Ok(message) = std::str::from_utf8(&buf[..n]) else {
                    error!("Client did not send valid utf8 message");
                    continue;
                };

                let message = message.trim_end_matches(|c: char| c.is_ascii_whitespace());

                info!("Received the string `{message}`");

                let _ = match message.split_once("=") {
                    Some((key, value)) => {
                        tx.send(Message::Insert(addr, key.to_owned(), value.to_owned()))
                    }
                    None => tx.send(Message::Retrieve(addr, message.to_owned())),
                };
            }
            Err(_) => {}
        }
    }
}
