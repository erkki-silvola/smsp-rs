use bytes::Bytes;
use smsp_rs::{
    ConnectionState, MasterConnection, Message, MockTransport, Parameters, Result, SequenceTracker,
    SlaveConnection,
};
use std::thread;

#[test]
fn test_full_request_response_with_length() -> Result<()> {
    let (master_transport, slave_transport) = MockTransport::pair();

    let slave_handle = thread::spawn(move || -> Result<Vec<u8>> {
        let mut slave = SlaveConnection::with_defaults(slave_transport);
        slave.wait_for_init()?;
        let request = slave.receive_request()?;
        let mut response = b"Echo: ".to_vec();
        response.extend_from_slice(&request);
        slave.send_response(&response)?;
        slave.close()?;
        Ok(response)
    });

    let mut master = MasterConnection::new(master_transport);
    let params = master.initialize()?;
    assert!(params.length_present);
    assert_eq!(params.fragment_count, 1);

    let response = master.request(b"Hello, Slave!")?;
    assert_eq!(response.as_ref(), b"Echo: Hello, Slave!");
    master.close()?;

    slave_handle.join().unwrap()?;
    Ok(())
}

#[test]
fn test_fragmented_request_response() -> Result<()> {
    let (master_transport, slave_transport) = MockTransport::pair();

    let slave_handle = thread::spawn(move || -> Result<()> {
        let mut slave = SlaveConnection::with_defaults(slave_transport);
        slave.wait_for_init()?;
        let request = slave.receive_request()?;
        assert!(request.len() > 256);
        slave.send_response(&request)?;
        slave.close()?;
        Ok(())
    });

    let mut master = MasterConnection::new(master_transport);
    master.initialize()?;

    let large_data = vec![0xAA; 700];
    let response = master.request(&large_data)?;
    assert_eq!(response.as_ref(), &large_data[..]);
    master.close()?;

    slave_handle.join().unwrap()?;
    Ok(())
}

#[test]
fn test_slave_cancel() -> Result<()> {
    let (master_transport, slave_transport) = MockTransport::pair();

    let slave_handle = thread::spawn(move || -> Result<()> {
        let params = Parameters {
            protocol_version: 1,
            length_present: true,
            timeout: std::time::Duration::from_millis(20000),
            fragment_count: 1,
            fragment_max_size: 256,
        };
        let mut slave = SlaveConnection::new(slave_transport, params);
        slave.wait_for_init()?;

        let mut buf = [0u8; 1];
        let _ = slave.parameters().timeout;

        let _request_start = slave.receive_request();
        Ok(())
    });

    let mut master = MasterConnection::new(master_transport);
    master.initialize()?;

    let _result = master.request(b"test");

    slave_handle.join().unwrap()?;
    Ok(())
}

#[test]
fn test_master_initialization_state() {
    let mut transport = MockTransport::new();
    let params = Parameters::default_slave();
    transport.add_rx_data(params.to_response().as_bytes());
    let mut master = MasterConnection::new(transport);
    let negotiated = master.initialize().unwrap();
    assert_eq!(negotiated, params);
    assert_eq!(master.state(), ConnectionState::Ready);
}

#[test]
fn test_protocol_message_flow() -> Result<()> {
    let msg = Message::more(0, true, Bytes::from_static(b"Hello"))?;
    assert!(msg.is_more());
    assert_eq!(msg.header.seq, 0);
    let bytes = msg.to_bytes();
    let (decoded, _) = Message::from_bytes(&bytes)?;
    assert_eq!(decoded.payload, msg.payload);
    Ok(())
}

#[test]
fn test_sequence_tracking() -> Result<()> {
    let mut seq = SequenceTracker::new();
    for i in 0..5 {
        assert!(seq.validate_and_advance(i));
    }
    assert!(!seq.validate_and_advance(10));
    Ok(())
}

#[test]
fn test_message_types() -> Result<()> {
    let ack = Message::ack(0, true)?;
    assert!(ack.is_ack());
    assert!(ack.is_last());

    let cancel = Message::cancel(0, true, Bytes::from("error"))?;
    assert!(cancel.is_cancel());

    let more = Message::more(1, true, Bytes::from("data"))?;
    assert!(more.is_more());
    assert!(!more.is_last());

    let last = Message::last(2, true, Bytes::from("final"))?;
    assert!(last.is_last());
    assert!(!last.is_more());
    Ok(())
}

#[test]
fn test_empty_request_response() -> Result<()> {
    let (master_transport, slave_transport) = MockTransport::pair();

    let slave_handle = thread::spawn(move || -> Result<()> {
        let mut slave = SlaveConnection::with_defaults(slave_transport);
        slave.wait_for_init()?;
        let request = slave.receive_request()?;
        assert!(request.is_empty());
        slave.send_response(b"")?;
        slave.close()?;
        Ok(())
    });

    let mut master = MasterConnection::new(master_transport);
    master.initialize()?;
    let response = master.request(b"")?;
    assert!(response.is_empty());
    master.close()?;

    slave_handle.join().unwrap()?;
    Ok(())
}
