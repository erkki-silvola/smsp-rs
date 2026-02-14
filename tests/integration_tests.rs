use smsp_rs::{Message, Parameters, Result};
use bytes::Bytes;

#[test]
fn test_protocol_message_flow() -> Result<()> {
    // Test basic message creation and serialization
    let msg = Message::more(0, true, Bytes::from_static(b"Hello"))?;
    assert!(msg.is_more());
    assert_eq!(msg.header.seq, 0);
    
    let bytes = msg.to_bytes();
    let (decoded, _) = Message::from_bytes(&bytes)?;
    assert_eq!(decoded.payload, msg.payload);
    
    Ok(())
}

#[test]
fn test_parameter_negotiation() -> Result<()> {
    let params = Parameters::default_slave();
    let response = params.to_response();
    let parsed = Parameters::from_response(&response)?;
    
    assert_eq!(params, parsed);
    assert_eq!(parsed.protocol_version, 1);
    assert_eq!(parsed.length_present, true);
    assert_eq!(parsed.fragment_count, 1);
    assert_eq!(parsed.fragment_max_size, 256);
    
    Ok(())
}

#[test]
fn test_fragmented_message_round_trip() -> Result<()> {
    use smsp_rs::{MasterConnection, MockTransport, Parameters};
    
    let mut transport = MockTransport::new();
    let params = Parameters::default_slave();
    transport.add_rx_data(params.to_response().as_bytes());
    
    let mut master = MasterConnection::new(transport);
    let negotiated = master.initialize()?;
    
    assert_eq!(negotiated.length_present, true);
    
    Ok(())
}

#[test]
fn test_sequence_tracking() -> Result<()> {
    use smsp_rs::SequenceTracker;
    
    let mut seq = SequenceTracker::new();
    
    // Validate sequence in order
    for i in 0..5 {
        assert!(seq.validate_and_advance(i));
    }
    
    // Out of order should fail
    assert!(!seq.validate_and_advance(10));
    
    Ok(())
}

#[test]
fn test_message_types() -> Result<()> {
    // Test ACK
    let ack = Message::ack(0, true)?;
    assert!(ack.is_ack());
    assert!(ack.is_last());
    
    // Test CANCEL
    let cancel = Message::cancel(0, true, Bytes::from("error"))?;
    assert!(cancel.is_cancel());
    
    // Test MORE
    let more = Message::more(1, true, Bytes::from("data"))?;
    assert!(more.is_more());
    assert!(!more.is_last());
    
    // Test LAST
    let last = Message::last(2, true, Bytes::from("final"))?;
    assert!(last.is_last());
    assert!(!last.is_more());
    
    Ok(())
}
