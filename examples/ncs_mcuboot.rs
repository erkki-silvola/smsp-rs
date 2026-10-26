#[path = "ncs/client.rs"]
mod client;
#[path = "ncs/rpc.rs"]
mod rpc;

use client::McubootClient;
use smsp_rs::{Error, Result, SerialTransport};
use std::env;
use std::path::Path;

fn serial_port_candidates(requested: &str) -> Vec<String> {
    if requested != "auto" {
        return vec![requested.to_string()];
    }
    let mut ports: Vec<String> = std::fs::read_dir("/dev")
        .ok()
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let name = entry.ok()?.file_name().to_string_lossy().into_owned();
            if name.starts_with("cu.usbmodem") {
                Some(format!("/dev/{name}"))
            } else {
                None
            }
        })
        .collect();
    ports.sort();
    ports.dedup();
    if ports.is_empty() {
        ports.push("/dev/ttyUSB0".into());
    }
    ports
}

async fn run_command(
    port: &str,
    baud: u32,
    cmd: &str,
    slot: u32,
    chunk_size: usize,
    upload_path: Option<&Path>,
) -> Result<()> {
    let transport = SerialTransport::open(port, baud)?;
    let mut client = McubootClient::new(transport);
    client.connect().await?;
    let chunk_size = if cmd == "upload" {
        if chunk_size == 0 {
            return Err(Error::Protocol("chunk_size must be greater than 0".into()));
        }
        if let Some(max) = client.max_upload_chunk_size() {
            if chunk_size > max {
                eprintln!("chunk_size {chunk_size} exceeds negotiated max {max}; using {max}");
                chunk_size.min(max)
            } else {
                chunk_size
            }
        } else {
            chunk_size
        }
    } else {
        chunk_size
    };
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
            let file = upload_path
                .ok_or_else(|| Error::Protocol("upload requires file path".into()))?;
            client.upload_image(file, slot, chunk_size).await?;
            println!("upload complete");
        }
        other => return Err(Error::Protocol(format!("unknown command: {other}"))),
    }
    client.close().await?;
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    let port = args.get(1).map_or("auto", |s| s.as_str());
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
    let upload_path = args.get(4).map(Path::new);
    let mut last_err = None;
    let auto_ports = port == "auto";
    for candidate in serial_port_candidates(port) {
        if auto_ports {
            eprintln!("trying {candidate}...");
        }
        match run_command(&candidate, baud, cmd, slot, chunk_size, upload_path).await {
            Ok(()) => return Ok(()),
            Err(e) => last_err = Some(e),
        }
    }
    Err(last_err.unwrap_or_else(|| Error::Protocol("no serial ports to try".into())))
}
