use super::rpc::{self, Request, Response};
use sha2::{Digest, Sha256};
use smsp_rs::{MasterConnection, Result, Transport};
use std::path::Path;

pub struct McubootClient<T: Transport> {
    master: MasterConnection<T>,
}

impl<T: Transport> McubootClient<T> {
    pub fn new(transport: T) -> Self {
        Self {
            master: MasterConnection::new(transport),
        }
    }

    pub async fn connect(&mut self) -> Result<()> {
        self.master.initialize().await?;
        Ok(())
    }

    pub async fn list_images(&mut self) -> Result<Vec<rpc::ImageInfo>> {
        let frame = rpc::encode_request(&Request::ListImages, &[])?;
        let body = self.master.request(&frame).await?;
        match rpc::decode_response(&body)? {
            (Response::ListImages { images }, _) => Ok(images),
            (Response::Error { message, .. }, _) => Err(smsp_rs::Error::Protocol(message)),
            _ => Err(smsp_rs::Error::Protocol(
                "unexpected response type for list_images".into(),
            )),
        }
    }

    pub async fn upload_image(&mut self, path: &Path, slot: u32, chunk_size: usize) -> Result<()> {
        let firmware = std::fs::read(path).map_err(smsp_rs::Error::Io)?;
        let sha256 = hex::encode(Sha256::digest(&firmware));
        let total = firmware.len() as u64;
        let mut offset = 0u64;
        while offset < total {
            let end = (offset as usize + chunk_size).min(firmware.len());
            let chunk = &firmware[offset as usize..end];
            let req = Request::UploadImage {
                slot,
                sha256: sha256.clone(),
                offset,
                length: chunk.len() as u64,
            };
            let frame = rpc::encode_request(&req, chunk)?;
            let body = self.master.request(&frame).await?;
            let (resp, _) = rpc::decode_response(&body)?;
            match resp {
                Response::UploadImage { offset: next, .. } => {
                    if next <= offset {
                        return Err(smsp_rs::Error::Protocol(
                            "device did not advance upload offset".into(),
                        ));
                    }
                    offset = next;
                }
                Response::Error { message, .. } => {
                    return Err(smsp_rs::Error::Protocol(message));
                }
                _ => {
                    return Err(smsp_rs::Error::Protocol(
                        "unexpected response type for upload_image".into(),
                    ));
                }
            }
        }
        Ok(())
    }

    pub async fn close(self) -> Result<()> {
        self.master.close().await
    }
}
