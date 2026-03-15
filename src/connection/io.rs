use crate::error::{Error, Result};
use crate::protocol::{Header, Message};
use crate::session::SequenceTracker;
use crate::transport::Transport;
use bytes::{Bytes, BytesMut};
use std::time::Duration;

pub(crate) fn read_exact<T: Transport>(
    transport: &mut T,
    buf: &mut [u8],
    timeout: Duration,
) -> Result<()> {
    let mut offset = 0;
    while offset < buf.len() {
        let n = transport.receive_timeout(&mut buf[offset..], timeout)?;
        if n == 0 {
            return Err(Error::Protocol("Unexpected EOF during read".into()));
        }
        offset += n;
    }
    Ok(())
}

pub(crate) fn read_message<T: Transport>(
    transport: &mut T,
    length_present: bool,
    timeout: Duration,
) -> Result<Message> {
    let mut hdr_byte = [0u8; 1];
    read_exact(transport, &mut hdr_byte, timeout)?;
    let header = Header::from_bytes(&hdr_byte)?;
    if length_present {
        let mut len_buf = [0u8; 2];
        read_exact(transport, &mut len_buf, timeout)?;
        let length = u16::from_be_bytes(len_buf);
        let mut payload = vec![0u8; length as usize];
        if length > 0 {
            read_exact(transport, &mut payload, timeout)?;
        }
        let full_header = Header::new(header.status, true, header.seq, Some(length))?;
        Message::new(full_header, Bytes::from(payload))
    } else {
        let mut buf = [0u8; 65535];
        let n = transport.receive_timeout(&mut buf, timeout)?;
        let payload = Bytes::copy_from_slice(&buf[..n]);
        let full_header = Header::new(header.status, false, header.seq, None)?;
        Message::new(full_header, payload)
    }
}

pub(crate) fn wait_ack<T: Transport>(
    transport: &mut T,
    length_present: bool,
    timeout: Duration,
) -> Result<()> {
    let msg = read_message(transport, length_present, timeout)?;
    if msg.is_cancel() {
        return Err(Error::Cancelled(
            String::from_utf8_lossy(&msg.payload).into(),
        ));
    }
    if !msg.is_ack() {
        return Err(Error::Protocol(format!(
            "Expected ACK, got status {:?}",
            msg.header.status
        )));
    }
    Ok(())
}

pub(crate) fn send_fragments<T: Transport>(
    transport: &mut T,
    data: &[u8],
    length_present: bool,
    fragment_count: u8,
    max_size: usize,
    timeout: Duration,
    sequence: &mut SequenceTracker,
) -> Result<()> {
    let chunks: Vec<&[u8]> = if length_present {
        data.chunks(max_size).collect()
    } else {
        vec![data]
    };
    let total = chunks.len();
    let mut sent_since_ack: u8 = 0;
    for (i, chunk) in chunks.iter().enumerate() {
        let is_last = i + 1 == total;
        let seq = sequence.next_seq();
        let msg = if is_last {
            Message::last(seq, length_present, Bytes::copy_from_slice(chunk))?
        } else {
            Message::more(seq, length_present, Bytes::copy_from_slice(chunk))?
        };
        transport.send(&msg.to_bytes())?;
        if is_last {
            break;
        }
        sent_since_ack += 1;
        if sent_since_ack >= fragment_count {
            wait_ack(transport, length_present, timeout)?;
            sent_since_ack = 0;
        }
    }
    Ok(())
}

pub(crate) fn receive_fragments<T: Transport>(
    transport: &mut T,
    length_present: bool,
    fragment_count: u8,
    timeout: Duration,
) -> Result<Bytes> {
    let mut seq = SequenceTracker::new();
    let mut response = BytesMut::new();
    let mut received_since_ack: u8 = 0;
    loop {
        let msg = read_message(transport, length_present, timeout)?;
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
        response.extend_from_slice(&msg.payload);
        if is_last {
            return Ok(response.freeze());
        }
        received_since_ack += 1;
        if received_since_ack >= fragment_count {
            let ack = Message::ack(0, length_present)?;
            transport.send(&ack.to_bytes())?;
            received_since_ack = 0;
        }
    }
}
