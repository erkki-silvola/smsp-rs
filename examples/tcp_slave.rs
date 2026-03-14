use smsp_rs::{Result, SlaveConnection, TcpTransport};
use std::env;
use std::net::TcpListener;

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    let addr = args.get(1).map_or("127.0.0.1:8080", |s| s.as_str());

    println!("SMSP Slave Server");
    println!("Listening on {addr}...");
    println!("Press Ctrl+C to stop");
    println!();

    let listener = TcpListener::bind(addr)?;

    for (num, stream) in listener.incoming().enumerate() {
        match stream {
            Ok(stream) => {
                let peer = stream.peer_addr()?;
                println!("[#{num}] Connection from {peer}");
                match handle_connection(stream) {
                    Ok(()) => println!("[#{num}] Closed successfully"),
                    Err(e) => eprintln!("[#{num}] Error: {e:?}"),
                }
                println!();
            }
            Err(e) => eprintln!("Accept error: {e:?}"),
        }
    }
    Ok(())
}

fn handle_connection(stream: std::net::TcpStream) -> Result<()> {
    let transport = TcpTransport::from_stream(stream)?;
    let mut slave = SlaveConnection::with_defaults(transport);

    println!("  Waiting for initialization...");
    slave.wait_for_init()?;
    println!("  Initialized with: {:?}", slave.parameters());

    loop {
        println!("  Waiting for request...");
        match slave.receive_request() {
            Ok(request) => {
                println!(
                    "  Request ({} bytes): {}",
                    request.len(),
                    String::from_utf8_lossy(&request)
                );
                let mut response = Vec::with_capacity(6 + request.len());
                response.extend_from_slice(b"Echo: ");
                response.extend_from_slice(&request);
                println!("  Sending response ({} bytes)...", response.len());
                slave.send_response(&response)?;
                println!("  Response sent");
                break;
            }
            Err(e) => {
                eprintln!("  Error receiving request: {e:?}");
                let _ = slave.send_error(&format!("{e:?}"));
                break;
            }
        }
    }

    slave.close()
}
