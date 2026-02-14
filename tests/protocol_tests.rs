use smsp_rs::{Header, Message, Parameters, Result, Status};
use bytes::Bytes;

#[test]
fn test_header_encoding() -> Result<()> {
    let header = Header::new(Status::More, true, 5, Some(256))?;
    let bytes = header.to_bytes();
    let decoded = Header::from_bytes(&bytes)?;
    
    assert_eq!(header, decoded);
    Ok(())
}

#[test]
fn test_message_encoding() -> Result<()> {
    let payload = Bytes::from_static(b"Test payload");
    let msg = Message::more(3, true, payload.clone())?;
    
    let bytes = msg.to_bytes();
    let (decoded, consumed) = Message::from_bytes(&bytes)?;
    
    assert_eq!(consumed, bytes.len());
    assert_eq!(decoded.header.seq, 3);
    assert_eq!(decoded.header.status, Status::More);
    assert_eq!(decoded.payload, payload);
    
    Ok(())
}

#[test]
fn test_parameters_parsing() -> Result<()> {
    let params_str = "1,1,20000,1,512\n";
    let params = Parameters::from_response(params_str)?;
    
    assert_eq!(params.protocol_version, 1);
    assert_eq!(params.length_present, true);
    assert_eq!(params.timeout.as_millis(), 20000);
    assert_eq!(params.fragment_count, 1);
    assert_eq!(params.fragment_max_size, 512);
    
    Ok(())
}

#[test]
fn test_parameters_formatting() -> Result<()> {
    let params = Parameters::default_slave();
    let formatted = params.to_response();
    let parsed = Parameters::from_response(&formatted)?;
    
    assert_eq!(params, parsed);
    Ok(())
}

#[test]
fn test_ack_message() -> Result<()> {
    let ack = Message::ack(7, true)?;
    
    assert!(ack.is_ack());
    assert_eq!(ack.header.seq, 7);
    assert_eq!(ack.header.status, Status::Last);
    assert_eq!(ack.payload.len(), 0);
    
    Ok(())
}

#[test]
fn test_cancel_message() -> Result<()> {
    let cancel = Message::cancel(2, true, Bytes::from_static(b"Error reason"))?;
    
    assert!(cancel.is_cancel());
    assert_eq!(cancel.header.seq, 2);
    assert_eq!(cancel.payload, Bytes::from_static(b"Error reason"));
    
    Ok(())
}

#[test]
fn test_status_types() {
    assert_eq!(Status::Last.to_bits(), 0b00);
    assert_eq!(Status::More.to_bits(), 0b01);
    assert_eq!(Status::Cancel.to_bits(), 0b10);
    assert_eq!(Status::Reserved.to_bits(), 0b11);
}

#[test]
fn test_sequence_wraparound() -> Result<()> {
    // Create 16 messages with sequence numbers 0-15
    for seq in 0..16 {
        let msg = Message::more(seq % 16, true, Bytes::from_static(b"test"))?;
        assert_eq!(msg.header.seq, seq % 16);
    }
    
    Ok(())
}

#[test]
fn test_large_payload() -> Result<()> {
    let large_payload = Bytes::from(vec![0xAB; 65535]);
    let msg = Message::last(10, true, large_payload.clone())?;
    
    assert_eq!(msg.payload.len(), 65535);
    assert_eq!(msg.header.length, Some(65535));
    
    let bytes = msg.to_bytes();
    let (decoded, _) = Message::from_bytes(&bytes)?;
    
    assert_eq!(decoded.payload, large_payload);
    
    Ok(())
}

#[test]
fn test_invalid_sequence() {
    // Sequence number > 15 should fail
    let result = Header::new(Status::More, true, 16, Some(100));
    assert!(result.is_err());
}

#[test]
fn test_length_present_validation() {
    // length_present true but no length should fail
    let result = Header::new(Status::More, true, 5, None);
    assert!(result.is_err());
    
    // length_present false but length provided should fail
    let result = Header::new(Status::More, false, 5, Some(100));
    assert!(result.is_err());
}
