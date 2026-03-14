use smsp_rs::{MasterConnection, Result, TcpTransport};
use std::env;

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    let addr = args.get(1).map_or("127.0.0.1:8080", |s| s.as_str());

    println!("Connecting to slave at {addr}...");
    let transport = TcpTransport::connect(addr)?;
    let mut master = MasterConnection::new(transport);

    println!("Initializing connection...");
    let params = master.initialize()?;
    println!("Connected! Parameters:");
    println!("  Protocol version: {}", params.protocol_version);
    println!("  Length present: {}", params.length_present);
    println!("  Timeout: {:?}", params.timeout);
    println!("  Fragment count: {}", params.fragment_count);
    println!("  Fragment max size: {} bytes", params.fragment_max_size);
    println!();

    println!("Sending request: 'Hello, Slave!'");
    let response = master.request(b"Hello, Slave!")?;
    println!(
        "Response ({} bytes): {}",
        response.len(),
        String::from_utf8_lossy(&response)
    );
    println!();

    println!("Sending larger request to test fragmentation...");
    let large_data =
        "This is a larger message that will be split into multiple fragments. ".repeat(10);
    println!("  Data size: {} bytes", large_data.len());
    let response = master.request(large_data.as_bytes())?;
    println!(
        "Response ({} bytes): {}",
        response.len(),
        String::from_utf8_lossy(&response)
    );
    println!();

    println!("Closing connection...");
    master.close()?;
    println!("Done.");
    Ok(())
}
