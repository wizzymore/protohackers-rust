use std::{collections::HashMap, net::SocketAddr, sync::Arc};

use lazy_static::lazy_static;
use log::{error, info};
use tokio::{net::UdpSocket, sync::RwLock};

const VERSION: &str = "Ken's Key-Value Store 1.0\n";

lazy_static! {
    static ref DATA: RwLock<HashMap<String, String>> = RwLock::new(HashMap::new());
}

async fn handle_client(socket: Arc<UdpSocket>, message: String, addr: SocketAddr) {
    match message.split_once("=") {
        Some((key, value)) => match key {
            "version" => {}
            key => {
                let mut data = DATA.write().await;
                data.insert(key.to_string(), value.to_string());
            }
        },
        None => match message.as_str() {
            "version" => {
                if socket.send_to(VERSION.as_bytes(), addr).await.is_err() {
                    error!("Failed to reply to {addr} about key `{message}`");
                }
            }
            key => {
                let data = DATA.read().await;
                let Some(value) = data.get(key) else {
                    info!("Client {addr} requested inexistent key `{key}`");
                    return;
                };
                if socket
                    .send_to(format!("{}\n", value).as_bytes(), addr)
                    .await
                    .is_err()
                {
                    error!("Failed to reply to {addr} about key `{message}`");
                }
            }
        },
    }
}

pub async fn run_unusual() {
    let socket = Arc::new(UdpSocket::bind("0.0.0.0:8080").await.unwrap());

    info!("🚀 Server listening on :8080");

    let mut buf = [0u8; 1000];
    loop {
        match socket.recv_from(&mut buf).await {
            Ok((n, addr)) => {
                info!("Received {n} bytes from {addr}");

                let Ok(message) = String::from_utf8(buf[0..n].trim_ascii().into()) else {
                    error!("Client did not send valid utf8 message");
                    continue;
                };

                let socket = socket.clone();
                tokio::spawn(async move { handle_client(socket, message, addr).await });
            }
            Err(_) => {}
        }
    }
}
