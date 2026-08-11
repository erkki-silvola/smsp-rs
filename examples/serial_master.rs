use smsp_rs::{MasterConnection, Result, SerialTransport};
use std::env;

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    let path = args.get(1).map_or("/dev/ttyUSB0", |s| s.as_str());
    let baud: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(115200);

    println!("Opening serial port {path} at {baud} baud...");
    let transport = SerialTransport::open(path, baud)?;
    let mut master = MasterConnection::new(transport);

    println!("Initializing connection...");
    let params = master.initialize().await?;
    println!("Connected! Parameters:");
    println!("  Protocol version: {}", params.protocol_version);
    println!("  Length present: {}", params.length_present);
    println!("  Timeout: {:?}", params.timeout);
    println!("  Fragment count: {}", params.fragment_count);
    println!("  Fragment max size: {} bytes", params.fragment_max_size);
    println!();

    println!("Sending request: 'Hello, Device!'");
    let response = master.request(b"Hello, Device!").await?;
    println!(
        "Response ({} bytes): {}",
        response.len(),
        String::from_utf8_lossy(&response)
    );

    println!("Closing connection...");
    master.close().await?;
    println!("Done.");
    Ok(())
}
