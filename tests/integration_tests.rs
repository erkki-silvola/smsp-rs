use bytes::{Bytes, BytesMut};
use smsp_rs::{
    Error, MasterConnection, Message, MockTransport, Parameters, Result, SequenceTracker,
    Transport, TransportCapabilities,
};
use std::time::Duration;

async fn read_exact(
    transport: &mut MockTransport,
    buf: &mut [u8],
    timeout: Duration,
) -> Result<()> {
    let mut offset = 0;
    while offset < buf.len() {
        let n = transport
            .receive_timeout(&mut buf[offset..], timeout)
            .await?;
        if n == 0 {
            return Err(Error::Protocol("Unexpected EOF during read".into()));
        }
        offset += n;
    }
    Ok(())
}

async fn read_message(
    transport: &mut MockTransport,
    length_present: bool,
    max_size: usize,
    timeout: Duration,
) -> Result<Message> {
    if length_present {
        let mut hdr = [0u8; 3];
        read_exact(transport, &mut hdr, timeout).await?;
        let header = smsp_rs::Header::from_bytes(&hdr)?;
        let length = header.length.unwrap_or(0);
        let mut payload = vec![0u8; length as usize];
        if length > 0 {
            read_exact(transport, &mut payload, timeout).await?;
        }
        Message::new(header, Bytes::from(payload))
    } else {
        let mut hdr_byte = [0u8; 1];
        read_exact(transport, &mut hdr_byte, timeout).await?;
        let header = smsp_rs::Header::from_bytes(&hdr_byte)?;
        let mut buf = vec![0u8; max_size];
        let n = transport
            .receive_no_length_payload(&mut buf, timeout)
            .await?;
        let payload = Bytes::copy_from_slice(&buf[..n]);
        Message::new(header, payload)
    }
}

async fn slave_wait_init(transport: &mut MockTransport, params: &Parameters) -> Result<()> {
    let mut buf = [0u8; 256];
    let n = transport.receive(&mut buf).await?;
    let request = std::str::from_utf8(&buf[..n])
        .map_err(|_| Error::InvalidParameters("Request is not valid UTF-8".into()))?;
    if request != Parameters::INIT_REQUEST {
        return Err(Error::Protocol(format!(
            "Expected '{}', got '{}'",
            Parameters::INIT_REQUEST,
            request
        )));
    }
    transport.send(params.to_response().as_bytes()).await?;
    Ok(())
}

async fn slave_receive_request(
    transport: &mut MockTransport,
    params: &Parameters,
) -> Result<Bytes> {
    let max_size = if params.length_present {
        params.fragment_max_size as usize
    } else {
        transport.capabilities().fragment_max_size as usize
    };
    let mut seq = SequenceTracker::new();
    let mut request = BytesMut::new();
    let mut received_since_ack: u8 = 0;
    loop {
        let msg = read_message(transport, params.length_present, max_size, params.timeout).await?;
        if msg.is_cancel() {
            return Err(Error::Cancelled(
                String::from_utf8_lossy(&msg.payload).into(),
            ));
        }
        if !seq.validate_and_advance(msg.header.seq) {
            return Err(Error::Sequence {
                expected: seq.current(),
                actual: msg.header.seq,
            });
        }
        let is_last = msg.is_last();
        request.extend_from_slice(&msg.payload);
        if is_last {
            return Ok(request.freeze());
        }
        received_since_ack += 1;
        if received_since_ack >= params.fragment_count {
            let ack = Message::ack(0, params.length_present)?;
            transport.send(&ack.to_bytes()).await?;
            received_since_ack = 0;
        }
    }
}

async fn slave_send_response(
    transport: &mut MockTransport,
    params: &Parameters,
    data: &[u8],
) -> Result<()> {
    let max_size = if params.length_present {
        params.fragment_max_size as usize
    } else {
        transport.capabilities().fragment_max_size as usize
    };
    let mut sequence = SequenceTracker::new();
    let chunks: Vec<&[u8]> = if data.is_empty() {
        vec![&[] as &[u8]]
    } else {
        data.chunks(max_size).collect()
    };
    let total = chunks.len();
    let mut sent_since_ack: u8 = 0;
    for (i, chunk) in chunks.iter().enumerate() {
        let is_last = i + 1 == total;
        let seq = sequence.next_seq();
        let msg = if is_last {
            Message::last(seq, params.length_present, Bytes::copy_from_slice(chunk))?
        } else {
            Message::more(seq, params.length_present, Bytes::copy_from_slice(chunk))?
        };
        transport.send(&msg.to_bytes()).await?;
        if is_last {
            break;
        }
        sent_since_ack += 1;
        if sent_since_ack >= params.fragment_count {
            let ack =
                read_message(transport, params.length_present, max_size, params.timeout).await?;
            if !ack.is_ack() {
                return Err(Error::Protocol("Expected ACK".into()));
            }
            sent_since_ack = 0;
        }
    }
    Ok(())
}

async fn run_echo_slave(mut transport: MockTransport, params: Parameters) -> Result<Vec<u8>> {
    slave_wait_init(&mut transport, &params).await?;
    let request = slave_receive_request(&mut transport, &params).await?;
    let mut response = b"Echo: ".to_vec();
    response.extend_from_slice(&request);
    slave_send_response(&mut transport, &params, &response).await?;
    transport.close().await?;
    Ok(response)
}

#[tokio::test]
async fn test_full_request_response_with_length() -> Result<()> {
    let (master_transport, slave_transport) = MockTransport::pair();
    let params = Parameters::default_slave();
    let slave_handle = tokio::spawn(run_echo_slave(slave_transport, params));

    let mut master = MasterConnection::new(master_transport);
    let params = master.initialize().await?;
    assert!(params.length_present);
    assert_eq!(params.fragment_count, 1);

    let response = master.request(b"Hello, Slave!").await?;
    assert_eq!(response.as_ref(), b"Echo: Hello, Slave!");
    master.close().await?;

    assert_eq!(slave_handle.await.unwrap()?, b"Echo: Hello, Slave!");
    Ok(())
}

#[tokio::test]
async fn test_fragmented_request_response() -> Result<()> {
    let (master_transport, mut slave_transport) = MockTransport::pair();
    let params = Parameters::default_slave();
    let slave_handle = tokio::spawn(async move {
        slave_wait_init(&mut slave_transport, &params).await?;
        let request = slave_receive_request(&mut slave_transport, &params).await?;
        assert!(request.len() > 256);
        slave_send_response(&mut slave_transport, &params, &request).await?;
        slave_transport.close().await?;
        Ok::<(), Error>(())
    });

    let mut master = MasterConnection::new(master_transport);
    master.initialize().await?;

    let large_data = vec![0xAA; 700];
    let response = master.request(&large_data).await?;
    assert_eq!(response.as_ref(), &large_data[..]);
    master.close().await?;

    slave_handle.await.unwrap()?;
    Ok(())
}

#[tokio::test]
async fn test_remote_cancel() -> Result<()> {
    let (master_transport, mut slave_transport) = MockTransport::pair();
    let params = Parameters::default_slave();
    let slave_handle = tokio::spawn(async move {
        slave_wait_init(&mut slave_transport, &params).await?;
        let _request = slave_receive_request(&mut slave_transport, &params).await?;
        let cancel = Message::cancel(0, params.length_present, Bytes::from("error"))?;
        slave_transport.send(&cancel.to_bytes()).await?;
        slave_transport.close().await?;
        Ok::<(), Error>(())
    });

    let mut master = MasterConnection::new(master_transport);
    master.initialize().await?;
    let result = master.request(b"test").await;
    assert!(matches!(result, Err(Error::Cancelled(_))));

    slave_handle.await.unwrap()?;
    Ok(())
}

#[tokio::test]
async fn test_protocol_message_flow() -> Result<()> {
    let msg = Message::more(0, true, Bytes::from_static(b"Hello"))?;
    assert!(msg.is_more());
    assert_eq!(msg.header.seq, 0);
    let bytes = msg.to_bytes();
    let (decoded, _) = Message::from_bytes(&bytes)?;
    assert_eq!(decoded.payload, msg.payload);
    Ok(())
}

#[tokio::test]
async fn test_sequence_tracking() -> Result<()> {
    let mut seq = SequenceTracker::new();
    for i in 0..5 {
        assert!(seq.validate_and_advance(i));
    }
    assert!(!seq.validate_and_advance(10));
    Ok(())
}

#[tokio::test]
async fn test_message_types() -> Result<()> {
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

#[tokio::test]
async fn test_no_length_fragmented_request_response() -> Result<()> {
    let caps = TransportCapabilities {
        length_present: false,
        fragment_max_size: 64,
    };
    let (master_transport, slave_transport) = MockTransport::pair();
    let master_transport = master_transport.with_capabilities(caps);
    let slave_transport = slave_transport.with_capabilities(caps);
    let params = Parameters::from_response("1,0,20000,2,0\n")?;
    let slave_handle = tokio::spawn(run_echo_slave(slave_transport, params));

    let mut master = MasterConnection::new(master_transport);
    let negotiated = master.initialize().await?;
    assert!(!negotiated.length_present);
    assert_eq!(negotiated.fragment_count, 2);

    let large_data = vec![0xBB; 200];
    let response = master.request(&large_data).await?;
    let mut expected = b"Echo: ".to_vec();
    expected.extend_from_slice(&large_data);
    assert_eq!(response.as_ref(), &expected[..]);
    master.close().await?;

    slave_handle.await.unwrap()?;
    Ok(())
}

#[tokio::test]
async fn test_empty_request_response() -> Result<()> {
    let (master_transport, mut slave_transport) = MockTransport::pair();
    let params = Parameters::default_slave();
    let slave_handle = tokio::spawn(async move {
        slave_wait_init(&mut slave_transport, &params).await?;
        let request = slave_receive_request(&mut slave_transport, &params).await?;
        assert!(request.is_empty());
        slave_send_response(&mut slave_transport, &params, b"").await?;
        slave_transport.close().await?;
        Ok::<(), Error>(())
    });

    let mut master = MasterConnection::new(master_transport);
    master.initialize().await?;
    let response = master.request(b"").await?;
    assert!(response.is_empty());
    master.close().await?;

    slave_handle.await.unwrap()?;
    Ok(())
}
