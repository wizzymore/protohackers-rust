use std::{env, net::SocketAddr, sync::Arc};

use chat::ChatImpl;
use log::{error, info};
use tokio::net::{TcpListener, TcpStream};

mod chat;

trait ServerImpl {
    fn new() -> Self;
    async fn handle_client(&self, stream: TcpStream, addr: SocketAddr);
}

#[tokio::main]
async fn main() {
    env_logger::builder()
        .filter_level(log::LevelFilter::Debug)
        .format_target(false)
        .format_timestamp(None)
        .init();
    let listener = TcpListener::bind("0.0.0.0:8080")
        .await
        .unwrap_or_else(|e| panic!("Could not bind listener: {e}"));

    info!("🚀 Server listening on :8080");

    let mut args = env::args();

    let command = match args.nth(1) {
        Some(command) => command,
        None => String::from("chat"),
    };

    let server_impl = match command.as_str() {
        "chat" => ChatImpl::new(),
        _ => {
            panic!("Invalid server implementation specified: {command}");
        }
    };

    let server_arc = Arc::new(server_impl);

    loop {
        match listener.accept().await {
            Ok((stream, addr)) => {
                let server = server_arc.clone();
                tokio::spawn(async move { server.handle_client(stream, addr).await });
            }
            Err(e) => {
                error!("Could not accept connection: {e}");
            }
        }
    }
}
