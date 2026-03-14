use bytes::Bytes;
use smsp_rs::{Header, Message, Parameters, Result, Status};

#[test]
fn test_header_encoding_roundtrip() -> Result<()> {
    let header = Header::new(Status::More, true, 5, Some(256))?;
    let decoded = Header::from_bytes(&header.to_bytes())?;
    assert_eq!(header, decoded);
    Ok(())
}

#[test]
fn test_header_without_length_roundtrip() -> Result<()> {
    let header = Header::new(Status::Cancel, false, 12, None)?;
    let decoded = Header::from_bytes(&header.to_bytes())?;
    assert_eq!(header, decoded);
    Ok(())
}

#[test]
fn test_message_encoding_roundtrip() -> Result<()> {
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
    let params = Parameters::from_response("1,1,20000,1,512\n")?;
    assert_eq!(params.protocol_version, 1);
    assert!(params.length_present);
    assert_eq!(params.timeout.as_millis(), 20000);
    assert_eq!(params.fragment_count, 1);
    assert_eq!(params.fragment_max_size, 512);
    Ok(())
}

#[test]
fn test_parameters_no_length_mode() -> Result<()> {
    let params = Parameters::from_response("1,0,30000,4,0\n")?;
    assert!(!params.length_present);
    assert_eq!(params.fragment_count, 4);
    assert_eq!(params.fragment_max_size, 0);
    Ok(())
}

#[test]
fn test_parameters_roundtrip() -> Result<()> {
    let params = Parameters::default_slave();
    let parsed = Parameters::from_response(&params.to_response())?;
    assert_eq!(params, parsed);
    Ok(())
}

#[test]
fn test_ack_message() -> Result<()> {
    let ack = Message::ack(7, true)?;
    assert!(ack.is_ack());
    assert_eq!(ack.header.seq, 7);
    assert_eq!(ack.header.status, Status::Last);
    assert!(ack.payload.is_empty());
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
fn test_status_bits() {
    assert_eq!(Status::Last.to_bits(), 0b00);
    assert_eq!(Status::More.to_bits(), 0b01);
    assert_eq!(Status::Cancel.to_bits(), 0b10);
    assert_eq!(Status::Reserved.to_bits(), 0b11);
}

#[test]
fn test_large_payload() -> Result<()> {
    let large_payload = Bytes::from(vec![0xAB; 65535]);
    let msg = Message::last(10, true, large_payload.clone())?;
    assert_eq!(msg.header.length, Some(65535));
    let bytes = msg.to_bytes();
    let (decoded, _) = Message::from_bytes(&bytes)?;
    assert_eq!(decoded.payload, large_payload);
    Ok(())
}

#[test]
fn test_invalid_sequence_rejected() {
    assert!(Header::new(Status::More, true, 16, Some(100)).is_err());
}

#[test]
fn test_length_present_validation() {
    assert!(Header::new(Status::More, true, 5, None).is_err());
    assert!(Header::new(Status::More, false, 5, Some(100)).is_err());
}

#[test]
fn test_sequence_wraparound() -> Result<()> {
    for seq in 0..16u8 {
        let msg = Message::more(seq, true, Bytes::from_static(b"test"))?;
        assert_eq!(msg.header.seq, seq);
    }
    Ok(())
}
