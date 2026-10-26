#![cfg(feature = "mcuboot-example")]

#[path = "../examples/ncs/client.rs"]
mod client;
#[path = "../examples/ncs/mock_device.rs"]
mod mock_device;
#[path = "../examples/ncs/rpc.rs"]
mod rpc;

use bytes::{Bytes, BytesMut};
use client::McubootClient;
use mock_device::MockDevice;
use rpc::Response;
use sha2::{Digest, Sha256};
use smsp_rs::{
    Error, MasterConnection, Message, MockTransport, Parameters, Result, SequenceTracker, Transport,
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
    let max_size = params.fragment_payload_limit(transport.capabilities());
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
    let max_size = params.fragment_payload_limit(transport.capabilities());
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

async fn run_mcuboot_slave(
    mut transport: MockTransport,
    mut device: MockDevice,
    max_requests: usize,
) -> Result<()> {
    let params = Parameters::default_slave();
    slave_wait_init(&mut transport, &params).await?;
    for _ in 0..max_requests {
        let request = slave_receive_request(&mut transport, &params).await;
        let body = match request {
            Ok(b) => b,
            Err(_) => break,
        };
        let response = device.handle(&body)?;
        slave_send_response(&mut transport, &params, &response).await?;
    }
    transport.close().await?;
    Ok(())
}

#[tokio::test]
async fn mcuboot_rpc_list_images_roundtrip() -> Result<()> {
    let (master_transport, slave_transport) = MockTransport::pair();
    let device = MockDevice::new();
    let slave = tokio::spawn(run_mcuboot_slave(slave_transport, device, 1));

    let mut client = McubootClient::new(master_transport);
    client.connect().await?;
    let images = client.list_images().await?;
    assert!(images.is_empty());
    client.close().await?;
    slave.await.unwrap()?;
    Ok(())
}

#[tokio::test]
async fn mcuboot_rpc_upload_chunk_size_capped_to_fragment_max() -> Result<()> {
    let firmware = vec![0xABu8; 400];
    let dir = std::env::temp_dir().join(format!("smsp_mcuboot_cap_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("fw.bin");
    std::fs::write(&path, &firmware).unwrap();

    let (master_transport, slave_transport) = MockTransport::pair();
    let device = MockDevice::new();
    let slave = tokio::spawn(run_mcuboot_slave(slave_transport, device, 3));

    let mut client = McubootClient::new(master_transport);
    client.connect().await?;
    assert_eq!(client.max_upload_chunk_size(), Some(256));
    client.upload_image(&path, 0, 512).await?;
    client.close().await?;
    let _ = std::fs::remove_dir_all(&dir);
    slave.await.unwrap()?;
    Ok(())
}

#[tokio::test]
async fn mcuboot_rpc_upload_multichunk_and_list() -> Result<()> {
    let firmware = vec![0xCDu8; 700];
    let sha = hex::encode(Sha256::digest(&firmware));
    let dir = std::env::temp_dir().join(format!("smsp_mcuboot_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("fw.bin");
    std::fs::write(&path, &firmware).unwrap();

    let (master_transport, slave_transport) = MockTransport::pair();
    let device = MockDevice::new();
    let slave = tokio::spawn(run_mcuboot_slave(slave_transport, device, 4));

    let mut client = McubootClient::new(master_transport);
    client.connect().await?;
    client.upload_image(&path, 0, 256).await?;
    let images = client.list_images().await?;
    assert_eq!(images.len(), 1);
    assert_eq!(images[0].hash, sha);
    client.close().await?;
    let _ = std::fs::remove_dir_all(&dir);
    slave.await.unwrap()?;
    Ok(())
}

#[tokio::test]
async fn mcuboot_rpc_upload_rejects_bad_offset() -> Result<()> {
    let (master_transport, mut slave_transport) = MockTransport::pair();
    let device = MockDevice::new();
    let params = Parameters::default_slave();
    let slave = tokio::spawn(async move {
        slave_wait_init(&mut slave_transport, &params).await?;
        let request = slave_receive_request(&mut slave_transport, &params).await?;
        let mut device = device;
        let response = device.handle(&request)?;
        slave_send_response(&mut slave_transport, &params, &response).await?;
        slave_transport.close().await?;
        Ok::<(), Error>(())
    });

    let mut master = MasterConnection::new(master_transport);
    master.initialize().await?;
    let bad = rpc::encode_request(
        &rpc::Request::UploadImage {
            slot: 0,
            sha256: "aa".repeat(32),
            total: 128,
            offset: 64,
            length: 0,
        },
        &[],
    )?;
    let body = master.request(&bad).await?;
    let (resp, _) = rpc::decode_response(&body)?;
    assert!(matches!(resp, Response::Error { code: 3, .. }));
    master.close().await?;
    slave.await.unwrap()?;
    Ok(())
}

#[tokio::test]
async fn mcuboot_rpc_invalid_frame_returns_error_envelope() -> Result<()> {
    let (master_transport, mut slave_transport) = MockTransport::pair();
    let device = MockDevice::new();
    let params = Parameters::default_slave();
    let slave = tokio::spawn(async move {
        slave_wait_init(&mut slave_transport, &params).await?;
        let request = slave_receive_request(&mut slave_transport, &params).await?;
        let mut device = device;
        let response = device.handle(&request)?;
        slave_send_response(&mut slave_transport, &params, &response).await?;
        slave_transport.close().await?;
        Ok::<(), Error>(())
    });

    let mut master = MasterConnection::new(master_transport);
    master.initialize().await?;
    let body = master.request(&[0, 50, 0xFF]).await?;
    let (resp, _) = rpc::decode_response(&body)?;
    assert!(matches!(resp, Response::Error { code: 2, .. }));
    master.close().await?;
    slave.await.unwrap()?;
    Ok(())
}

#[test]
fn mcuboot_rpc_truncated_header_rejected() {
    let mut frame = rpc::encode_request(&rpc::Request::ListImages, &[]).unwrap();
    frame.truncate(3);
    assert!(rpc::decode_request(&frame).is_err());
}
