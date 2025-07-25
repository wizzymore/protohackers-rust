use std::env;

use chat::run_chat;
use speed::run_speed;
use unusual::run_unusual;

mod chat;
mod speed;
mod unusual;

#[tokio::main]
async fn main() {
    env_logger::builder()
        .filter_level(log::LevelFilter::Debug)
        .format_target(false)
        .format_timestamp(None)
        .init();

    let mut args = env::args();

    let command = match args.nth(1) {
        Some(command) => command,
        None => String::from("chat"),
    };

    match command.as_str() {
        "chat" => run_chat().await,
        "unusual" => run_unusual().await,
        "speed" => run_speed().await,
        _ => {
            panic!("Invalid server implementation specified: {command}");
        }
    };
}
