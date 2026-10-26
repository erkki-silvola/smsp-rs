use super::rpc::{self, Request, Response};
use sha2::{Digest, Sha256};
use smsp_rs::Result;
use std::collections::HashMap;

struct UploadState {
    received: Vec<u8>,
}

pub struct MockDevice {
    uploads: HashMap<(u32, String), UploadState>,
    expected_totals: HashMap<(u32, String), u64>,
    listed_images: Vec<rpc::ImageInfo>,
}

impl MockDevice {
    pub fn new() -> Self {
        Self {
            uploads: HashMap::new(),
            expected_totals: HashMap::new(),
            listed_images: Vec::new(),
        }
    }

    pub fn expect_upload(&mut self, slot: u32, sha256: &str, total: u64) {
        self.expected_totals
            .insert((slot, sha256.to_owned()), total);
    }

    pub fn handle(&mut self, frame: &[u8]) -> Result<Vec<u8>> {
        let (req, data) = match rpc::decode_request(frame) {
            Ok(v) => v,
            Err(_) => {
                return rpc::encode_response(&rpc::error_response(None, 2, "invalid frame"), &[]);
            }
        };
        let resp = match req {
            Request::ListImages => Response::ListImages {
                images: self.listed_images.clone(),
            },
            Request::UploadImage {
                slot,
                sha256,
                offset,
                length,
            } => self.upload_chunk(slot, sha256, offset, length, &data)?,
        };
        rpc::encode_response(&resp, &[])
    }

    fn upload_chunk(
        &mut self,
        slot: u32,
        sha256: String,
        offset: u64,
        length: u64,
        data: &[u8],
    ) -> Result<Response> {
        if length as usize != data.len() {
            return Ok(rpc::error_response(
                Some("upload_image"),
                4,
                "length mismatch",
            ));
        }
        let key = (slot, sha256.clone());
        let state = self.uploads.entry(key.clone()).or_insert(UploadState {
            received: Vec::new(),
        });
        if offset != state.received.len() as u64 {
            return Ok(rpc::error_response(
                Some("upload_image"),
                3,
                "offset mismatch",
            ));
        }
        state.received.extend_from_slice(data);
        let image_size = state.received.len() as u64;
        let remaining = self
            .expected_totals
            .get(&key)
            .map(|t| t.saturating_sub(image_size))
            .unwrap_or(0);
        if remaining == 0 {
            if let Some(total) = self.expected_totals.get(&key) {
                if image_size == *total {
                    self.listed_images.push(rpc::ImageInfo {
                        slot,
                        version: 1,
                        hash: hex::encode(Sha256::digest(&state.received)),
                        active: false,
                    });
                }
            }
        }
        Ok(Response::UploadImage {
            offset: image_size,
            remaining,
        })
    }
}

impl Default for MockDevice {
    fn default() -> Self {
        Self::new()
    }
}
