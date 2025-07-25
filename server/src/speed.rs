use std::{collections::HashMap, net::SocketAddr, sync::Arc};

use log::{error, info};
use server_macros::Packet;
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream, tcp::OwnedWriteHalf},
    sync::{
        Mutex,
        mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel},
    },
};

trait Packet: Sized + Send + Sync {
    const OPCODE: u8;

    async fn deserialize<R: AsyncRead + Unpin>(reader: &mut R) -> Result<Self, std::io::Error>;
}

#[derive(Debug, Packet)]
#[opcode = 0x10]
struct ErrorPacket {
    message: String,
}

#[derive(Debug, Packet)]
#[opcode = 0x20]
struct PlatePacket {
    plate: String,
    timestamp: u32,
}

#[derive(Debug, Packet)]
#[opcode = 0x21]
struct TicketPacket {
    plate: String,
    road: u16,
    mile1: u16,
    timestamp1: u32,
    mile2: u16,
    timestamp2: u32,
    speed: u16, // 100x miles per hour
}

#[derive(Debug, Packet)]
#[opcode = 0x40]
struct WantHeartBeatPacket {
    interval: u32, // in deciseconds
}

#[derive(Debug, Packet)]
#[opcode = 0x41]
struct HeartBeatPacket {}

#[derive(Debug, Packet)]
#[opcode = 0x80]
struct Camera {
    road: u16,
    mile: u16,
    limit: u16, // miles per hour
}

#[derive(Debug, Packet)]
#[opcode = 0x81]
struct Dispatcher {
    numroads: u8,
    roads: Vec<u16>, // miles per hour
}

enum MessageType {
    ClientConnected(OwnedWriteHalf, SocketAddr),
    ClientDisconnected(SocketAddr),
    Plate(SocketAddr, PlatePacket),
    WantHeartBeat(SocketAddr, WantHeartBeatPacket),
    IAmCamera(SocketAddr, Camera),
    IAmDispatcher(SocketAddr, Dispatcher),
}

async fn handle_client(tx: UnboundedSender<MessageType>, stream: TcpStream, addr: SocketAddr) {
    let (mut read, write) = stream.into_split();

    _ = tx.send(MessageType::ClientConnected(write, addr));

    loop {
        let Ok(n) = read.read_u8().await else {
            error!("Could not read from connection {addr}");
            _ = tx.send(MessageType::ClientDisconnected(addr));
            break;
        };

        match n {
            PlatePacket::OPCODE => match PlatePacket::deserialize(&mut read).await {
                Ok(packet) => {
                    let _ = tx.send(MessageType::Plate(addr, packet));
                }
                Err(_) => {
                    error!("Could not deserialize packet");
                }
            },
            Camera::OPCODE => match Camera::deserialize(&mut read).await {
                Ok(packet) => {
                    let _ = tx.send(MessageType::IAmCamera(addr, packet));
                }
                Err(_) => {
                    error!("Could not deserialize packet");
                }
            },
            Dispatcher::OPCODE => match Dispatcher::deserialize(&mut read).await {
                Ok(packet) => {
                    let _ = tx.send(MessageType::IAmDispatcher(addr, packet));
                }
                Err(_) => {
                    error!("Could not deserialize packet");
                }
            },
            WantHeartBeatPacket::OPCODE => {
                match WantHeartBeatPacket::deserialize(&mut read).await {
                    Ok(packet) => {
                        let _ = tx.send(MessageType::WantHeartBeat(addr, packet));
                    }
                    Err(_) => {
                        error!("Could not deserialize packet");
                    }
                }
            }
            _ => {
                error!("Received unknown packet");
                break;
            }
        }
    }
}

async fn run_server(mut rx: UnboundedReceiver<MessageType>) {
    let mut cameras = HashMap::new();
    let mut dispatchers = HashMap::new();
    let mut sockets = HashMap::new();
    let mut heartbeats = HashMap::new();
    loop {
        match rx.recv().await {
            Some(packet) => {
                match packet {
                    MessageType::ClientConnected(write, addr) => {
                        sockets.insert(addr, Arc::new(Mutex::new(write)));
                    }
                    MessageType::ClientDisconnected(addr) => {
                        sockets.remove(&addr);
                        cameras.remove(&addr);
                        dispatchers.remove(&addr);
                    }
                    MessageType::IAmDispatcher(addr, packet) => {
                        dispatchers.insert(addr, packet);
                    }
                    MessageType::IAmCamera(addr, packet) => {
                        cameras.insert(addr, packet);
                    }
                    MessageType::Plate(addr, plate) => {
                        todo!();
                    }
                    MessageType::WantHeartBeat(addr, packet) => {
                        let Some(write) = sockets.get(&addr) else {
                            error!("Client requested heart beat but doesn't appear connected");
                            continue;
                        };
                        heartbeats.insert(addr, tokio::spawn(handle_heartbeat(write.clone())));
                    }
                };
            }
            None => {}
        }
    }
}

async fn handle_heartbeat(write: Arc<Mutex<OwnedWriteHalf>>) {}

pub async fn run_speed() {
    let listener = Arc::new(TcpListener::bind("0.0.0.0:8080").await.unwrap());

    info!("🚀 Server listening on :8080");

    let (tx, rx) = unbounded_channel::<MessageType>();

    tokio::spawn(run_server(rx));

    loop {
        match listener.accept().await {
            Ok((stream, addr)) => {
                tokio::spawn(handle_client(tx.clone(), stream, addr));
            }
            Err(e) => {
                error!("Could not accept connection: {e}");
            }
        }
    }
}
