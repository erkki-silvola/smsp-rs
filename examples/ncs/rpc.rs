use serde::{Deserialize, Serialize};
use smsp_rs::{Error, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Request {
    ListImages,
    UploadImage {
        slot: u32,
        sha256: String,
        total: u64,
        offset: u64,
        length: u64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageInfo {
    pub slot: u32,
    pub version: u32,
    pub hash: String,
    pub active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Response {
    ListImages {
        images: Vec<ImageInfo>,
    },
    UploadImage {
        offset: u64,
        remaining: u64,
    },
    Error {
        #[serde(rename = "for")]
        for_op: Option<String>,
        code: i32,
        message: String,
    },
}

pub fn encode_request(req: &Request, data: &[u8]) -> Result<Vec<u8>> {
    encode_frame(req, data)
}

pub fn encode_response(resp: &Response, data: &[u8]) -> Result<Vec<u8>> {
    encode_frame(resp, data)
}

fn encode_cbor<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    ciborium::ser::into_writer(value, &mut out)
        .map_err(|e| Error::Protocol(format!("CBOR encode: {e}")))?;
    Ok(out)
}

fn encode_frame<T: Serialize>(header: &T, data: &[u8]) -> Result<Vec<u8>> {
    let header_bytes = encode_cbor(header)?;
    if header_bytes.len() > u16::MAX as usize {
        return Err(Error::Protocol("RPC header too large".into()));
    }
    let mut out = Vec::with_capacity(2 + header_bytes.len() + data.len());
    out.extend_from_slice(&(header_bytes.len() as u16).to_be_bytes());
    out.extend_from_slice(&header_bytes);
    out.extend_from_slice(data);
    Ok(out)
}

pub fn decode_request(bytes: &[u8]) -> Result<(Request, Vec<u8>)> {
    decode_frame(bytes)
}

pub fn decode_response(bytes: &[u8]) -> Result<(Response, Vec<u8>)> {
    decode_frame(bytes)
}

fn decode_frame<T: for<'de> Deserialize<'de>>(bytes: &[u8]) -> Result<(T, Vec<u8>)> {
    if bytes.len() < 2 {
        return Err(Error::Protocol("RPC frame too short".into()));
    }
    let header_len = u16::from_be_bytes([bytes[0], bytes[1]]) as usize;
    let header_end = 2 + header_len;
    if bytes.len() < header_end {
        return Err(Error::Protocol("RPC header length exceeds frame".into()));
    }
    let header: T = ciborium::de::from_reader(&bytes[2..header_end])
        .map_err(|e| Error::Protocol(format!("CBOR decode: {e}")))?;
    let data = bytes[header_end..].to_vec();
    Ok((header, data))
}

pub fn error_response(for_op: Option<&str>, code: i32, message: impl Into<String>) -> Response {
    Response::Error {
        for_op: for_op.map(str::to_owned),
        code,
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_decode_list_images_roundtrip() {
        let frame = encode_request(&Request::ListImages, &[]).unwrap();
        let (req, data) = decode_request(&frame).unwrap();
        assert!(matches!(req, Request::ListImages));
        assert!(data.is_empty());
    }

    #[test]
    fn decode_rejects_truncated_header() {
        let mut frame = encode_request(&Request::ListImages, &[]).unwrap();
        frame.truncate(3);
        assert!(decode_request(&frame).is_err());
    }

    #[test]
    fn upload_carries_binary_data_after_header() {
        let req = Request::UploadImage {
            slot: 0,
            sha256: "ab".repeat(32),
            total: 4,
            offset: 0,
            length: 4,
        };
        let frame = encode_request(&req, b"firm").unwrap();
        let (decoded, data) = decode_request(&frame).unwrap();
        assert!(matches!(decoded, Request::UploadImage { length: 4, .. }));
        assert_eq!(data, b"firm");
    }
}
