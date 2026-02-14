use smsp_rs::{Result, SlaveConnection, TcpTransport};
use std::env;
use std::net::TcpListener;

fn main() -> Result<()> {
    // Get bind address from command line or use default
    let args: Vec<String> = env::args().collect();
    let addr = if args.len() > 1 {
        args[1].clone()
    } else {
        "127.0.0.1:8080".to_string()
    };

    println!("SMSP Slave Server");
    println!("=================");
    println!();
    println!("Listening on {}...", addr);
    println!("Press Ctrl+C to stop");
    println!();

    let listener = TcpListener::bind(&addr)?;

    for (conn_num, stream) in listener.incoming().enumerate() {
        match stream {
            Ok(stream) => {
                let peer_addr = stream.peer_addr()?;
                println!("[Connection #{}] New connection from {}", conn_num + 1, peer_addr);

                match handle_connection(stream) {
                    Ok(_) => {
                        println!("[Connection #{}] Connection closed successfully", conn_num + 1);
                    }
                    Err(e) => {
                        eprintln!("[Connection #{}] Error: {:?}", conn_num + 1, e);
                    }
                }
                println!();
            }
            Err(e) => {
                eprintln!("Connection error: {:?}", e);
            }
        }
    }

    Ok(())
}

fn handle_connection(stream: std::net::TcpStream) -> Result<()> {
    // Create slave connection with default parameters
    let transport = TcpTransport::from_stream(stream)?;
    let mut slave = SlaveConnection::with_defaults(transport);

    // Wait for initialization
    println!("  Waiting for initialization...");
    slave.wait_for_init()?;
    println!("  Connection initialized with parameters:");
    let params = slave.parameters();
    println!("    Protocol version: {}", params.protocol_version);
    println!("    Length present: {}", params.length_present);
    println!("    Timeout: {:?}", params.timeout);
    println!("    Fragment count: {}", params.fragment_count);
    println!("    Fragment max size: {} bytes", params.fragment_max_size);

    // Handle multiple requests in a loop
    loop {
        // Receive request
        println!("  Waiting for request...");
        match slave.receive_request() {
            Ok(request) => {
                println!("  Received request ({} bytes):", request.len());
                println!("    {}", String::from_utf8_lossy(&request));

                // Process request and generate response
                let response = process_request(&request);
                
                println!("  Sending response ({} bytes)...", response.len());
                slave.send_response(&response)?;
                println!("  Response sent");

                // For this example, close after one request/response cycle
                break;
            }
            Err(e) => {
                eprintln!("  Error receiving request: {:?}", e);
                // Try to send error response
                let _ = slave.send_error(&format!("{:?}", e));
                break;
            }
        }
    }

    // Close connection
    slave.close()?;
    Ok(())
}

fn process_request(request: &[u8]) -> Vec<u8> {
    // Simple echo server with a prefix
    let mut response = Vec::new();
    response.extend_from_slice(b"Echo: ");
    response.extend_from_slice(request);
    response
}
