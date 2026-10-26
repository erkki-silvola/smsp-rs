#[path = "ncs/client.rs"]
mod client;
#[path = "ncs/rpc.rs"]
mod rpc;

use client::McubootClient;
use smsp_rs::{Result, SerialTransport};
use std::env;
use std::path::Path;

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    let port = args.get(1).map_or("/dev/ttyUSB0", |s| s.as_str());
    let baud: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(115200);
    let cmd = args.get(3).map(|s| s.as_str()).unwrap_or("list-images");
    let mut slot = 0u32;
    let mut chunk_size = 256usize;
    for i in 4..args.len() {
        match args[i].as_str() {
            "--slot" => {
                slot = args.get(i + 1).and_then(|s| s.parse().ok()).unwrap_or(0);
            }
            "--chunk-size" => {
                chunk_size = args.get(i + 1).and_then(|s| s.parse().ok()).unwrap_or(256);
            }
            _ => {}
        }
    }
    let transport = SerialTransport::open(port, baud)?;
    let mut client = McubootClient::new(transport);
    client.connect().await?;
    match cmd {
        "list-images" => {
            let images = client.list_images().await?;
            for img in images {
                println!(
                    "slot={} version={} active={} hash={}",
                    img.slot, img.version, img.active, img.hash
                );
            }
        }
        "upload" => {
            let file = args
                .get(4)
                .ok_or_else(|| smsp_rs::Error::Protocol("upload requires file path".into()))?;
            client
                .upload_image(Path::new(file), slot, chunk_size)
                .await?;
            println!("upload complete");
        }
        other => {
            return Err(smsp_rs::Error::Protocol(format!(
                "unknown command: {other}"
            )));
        }
    }
    client.close().await?;
    Ok(())
}
