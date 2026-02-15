use smsp_rs::{MasterConnection, Result, TcpTransport};
use std::env;

fn main() -> Result<()> {
    // Get server address from command line or use default
    let args: Vec<String> = env::args().collect();
    let addr = if args.len() > 1 {
        args[1].clone()
    } else {
        "127.0.0.1:8080".to_string()
    };

    println!("Connecting to slave at {}...", addr);

    // Connect to slave
    let transport = TcpTransport::connect(&addr)?;
    let mut master = MasterConnection::new(transport);

    // Initialize connection
    println!("Initializing connection...");
    let params = master.initialize()?;
    println!("Connection established!");
    println!("Parameters: {:?}", params);
    println!("  Protocol version: {}", params.protocol_version);
    println!("  Length present: {}", params.length_present);
    println!("  Timeout: {:?}", params.timeout);
    println!("  Fragment count: {}", params.fragment_count);
    println!("  Fragment max size: {} bytes", params.fragment_max_size);
    println!();

    // Send a simple request
    println!("Sending request: 'Hello, Slave!'");
    let request_data = b"Hello, Slave!";
    let response = master.request(request_data)?;

    println!("Received response ({} bytes):", response.len());
    println!("  {}", String::from_utf8_lossy(&response));
    println!();

    // Send a larger request to test fragmentation
    println!("Sending larger request to test fragmentation...");
    let large_data =
        "This is a larger message that will be split into multiple fragments. ".repeat(10);
    let large_data_bytes = large_data.as_bytes();
    println!("  Data size: {} bytes", large_data_bytes.len());

    let response = master.request(large_data_bytes)?;
    println!("Received response ({} bytes):", response.len());
    println!("  {}", String::from_utf8_lossy(&response));
    println!();

    // Close connection
    println!("Closing connection...");
    master.close()?;
    println!("Connection closed.");

    Ok(())
}
