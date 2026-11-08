//! Writer-side of the Matrix attachment encryption scheme.
//!
//! The SDK only ships a reader-side encryptor (`AttachmentEncryptor` wraps a
//! `Read`), but persisting plain (unencrypted-room) video downloads in the
//! on-disk cache needs to encrypt on the way *into* the file. This module
//! implements the matching [`std::io::Write`] side of the exact same scheme
//! (AES-256-CTR + SHA-256 of the ciphertext, v2 attachment info), so the
//! resulting files decrypt with the SDK's `AttachmentDecryptor` -- see the
//! round-trip test.

use std::io::Write;

use aes::{
    Aes256,
    cipher::{KeyIvInit, StreamCipher},
};
use ctr::Ctr128BE;
use matrix_sdk_base::crypto::MediaEncryptionInfo;
use ruma::events::room::{EncryptedFileHash, EncryptedFileHashes, V2EncryptedFileInfo};
use ruma::serde::Base64;
use sha2::{Digest, Sha256};

/// Encrypts everything written to the inner writer as a Matrix attachment.
///
/// After writing everything, [`AttachmentWriter::finish`] flushes and returns
/// the info needed for decryption.
pub struct AttachmentWriter<W: Write> {
    inner: W,
    cipher: Ctr128BE<Aes256>,
    sha: Sha256,
    key: [u8; 32],
    iv: [u8; 16],
}

impl<W: Write> AttachmentWriter<W> {
    /// Wraps the given writer, encrypting everything with a fresh key.
    pub fn new(inner: W) -> Result<Self, std::io::Error> {
        let mut key = [0u8; 32];
        let mut iv = [0u8; 16];
        getrandom::fill(&mut key)?;
        getrandom::fill(&mut iv)?;

        let cipher = <Ctr128BE<Aes256>>::new(&key.into(), &iv.into());

        Ok(Self {
            inner,
            cipher,
            sha: Sha256::default(),
            key,
            iv,
        })
    }

    /// Flushes the stream and returns the information needed for decryption.
    pub fn finish(mut self) -> Result<MediaEncryptionInfo, std::io::Error> {
        self.inner.flush()?;

        // Like the v2 attachment scheme, the hash is over the ciphertext.
        let hash: [u8; 32] = self.sha.finalize().into();
        let mut hashes = EncryptedFileHashes::new();
        hashes.insert(EncryptedFileHash::Sha256(Base64::new(hash)));

        Ok(MediaEncryptionInfo {
            encryption_info: V2EncryptedFileInfo::encode(self.key, self.iv).into(),
            hashes,
        })
    }
}

impl<W: Write> Write for AttachmentWriter<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let mut data = buf.to_vec();
        self.cipher.apply_keystream(&mut data);
        // Like the v2 attachment scheme, the hash is over the ciphertext.
        self.sha.update(&data);
        self.inner.write_all(&data)?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    /// Everything written through the writer must decrypt back with the SDK's
    /// own `AttachmentDecryptor` into exactly the input data.
    #[test]
    fn round_trips_through_sdk_decryptor() {
        let input: Vec<u8> = (0..500_000u32).map(|i| (i % 251) as u8).collect();

        let mut out = Vec::new();
        let mut writer = AttachmentWriter::new(&mut out).unwrap();
        // Write in odd-sized chunks to exercise the CTR block alignment.
        for chunk in input.chunks(1024 * 300 + 7) {
            writer.write_all(chunk).unwrap();
        }
        let info = writer.finish().unwrap();

        let mut cursor = std::io::Cursor::new(out);
        let mut decryptor =
            matrix_sdk_base::crypto::AttachmentDecryptor::new(&mut cursor, info).unwrap();
        // Reading to the end also verifies the info's sha256 hash of the
        // ciphertext.
        let mut decrypted = Vec::new();
        decryptor
            .read_to_end(&mut decrypted)
            .expect("hash should verify");
        assert_eq!(input, decrypted);
    }
}
