use std::{env, net::SocketAddr, sync::Arc};

use chat::ChatImpl;
use tokio::net::{TcpListener, TcpStream};
use tracing::{Level, error, info};

mod chat;

trait ServerImpl {
    fn new() -> Self;
    async fn handle_client(&self, stream: TcpStream, addr: SocketAddr);
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_ansi(false)
        .with_max_level(Level::TRACE)
        .without_time()
        .pretty()
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
                error!(err = e.to_string(), "Could not accept connection");
            }
        }
    }
}
