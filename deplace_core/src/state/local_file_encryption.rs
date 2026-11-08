//! Writer- and reader-side of the Matrix attachment encryption scheme.
//!
//! The SDK only ships a reader-side encryptor (`AttachmentEncryptor` wraps a
//! `Read`), but persisting plain (unencrypted-room) video downloads in the
//! on-disk cache needs to encrypt on the way *into* the file. This module
//! implements the matching [`std::io::Write`] side of the exact same scheme
//! (AES-256-CTR + SHA-256 of the ciphertext, v2 attachment info), so the
//! resulting files decrypt with the SDK's `AttachmentDecryptor` -- see the
//! round-trip test.
//!
//! On the read side, [`AttachmentReader`] decrypts like the SDK's decryptor
//! but is *seekable*: each byte position in the CTR keystream corresponds
//! 1:1 to a ciphertext offset, so jumping somewhere is a counter seek instead
//! of fast-forwarding through (and hash-digesting) all the skipped data --
//! decrypting the stream feeding GStreamer tens (or hundreds) of megabytes
//! before `moov` in skip-then-play workloads.

use std::io::{Read, Seek, SeekFrom, Write};

use aes::{
    Aes256,
    cipher::{KeyIvInit, StreamCipher, StreamCipherSeek},
};
use ctr::Ctr128BE;
use matrix_sdk_base::crypto::{DecryptorError, MediaEncryptionInfo};
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

/// Decrypts data read from the inner reader on the fly, like the SDK's
/// [`matrix_sdk_base::crypto::AttachmentDecryptor`], but seekable: the CTR
/// keystream position corresponds directly to the ciphertext offset, so
/// [`AttachmentReader::seek_to`] is an O(1) counter jump with no need to
/// read, decrypt or hash the skipped bytes. What costs a full-file pass with
/// the SDK's decryptor (e.g. seeking a non-faststart mp4 to its `moov` box at
/// the end of a big file) is instant here.
///
/// The v2 info's SHA-256 hash of the ciphertext is *not* verified: a seeked
/// reader only digests the bytes it actually reads, so its end-of-stream
/// comparison would compare a partial hash. Integrity is verified where all
/// bytes pass by anyway: when the cache file is written (the download loop
/// hashes its output).
pub struct AttachmentReader<R: Read + Seek> {
    inner: R,
    cipher: Ctr128BE<Aes256>,
}

impl<R: Read + Seek> AttachmentReader<R> {
    /// Wraps the given reader, decrypting all data read from it.
    pub fn new(inner: R, info: MediaEncryptionInfo) -> Result<Self, DecryptorError> {
        // The cipher setup mirrors the SDK's `AttachmentDecryptor::new`: of
        // the attachment encryption versions only v2 exists in practice --
        // all files the cache hands out decrypt with this scheme.
        let ruma::events::room::EncryptedFileInfo::V2(info) = info.encryption_info else {
            return Err(DecryptorError::UnknownVersion);
        };
        let cipher =
            <Ctr128BE<Aes256>>::new(info.k.as_inner().into(), info.iv.as_inner().into());
        Ok(Self { inner, cipher })
    }

    /// Repositions the stream to the given plaintext/ciphertext offset.
    pub fn seek_to(&mut self, offset: u64) -> std::io::Result<()> {
        // Ctr128BE's keystream is addressable in bytes: `seek` also covers
        // offsets within an AES block, no block alignment is needed.
        self.cipher.seek(offset as usize);
        self.inner.seek(SeekFrom::Start(offset))?;
        Ok(())
    }
}

impl<R: Read + Seek> Read for AttachmentReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let read = self.inner.read(buf)?;
        self.cipher.apply_keystream(&mut buf[..read]);
        Ok(read)
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

    /// Seeks must decrypt into exactly the plaintext at that offset, both
    /// block-aligned and unaligned, backwards and forwards, and matches the
    /// SDK's sequential decryptor byte for byte.
    #[test]
    fn seek_decrypts_like_sequential_reads() {
        // Multi-block input with irregular (non-repeating) content.
        let input: Vec<u8> = (0..2_000_000u64)
            .map(|i| (i * 2654435761 % 1000003) as u8)
            .collect();

        let mut out = Vec::new();
        let mut writer = AttachmentWriter::new(&mut out).unwrap();
        writer.write_all(&input).unwrap();
        let info = writer.finish().unwrap();

        // Seeks to observe, both block-aligned and unaligned, forwards and
        // backwards over the same reader.
        let seeks: [(u64, u64); 7] = [
            (0, 1),
            (16, 1000),
            (512 * 1024, 70001),
            (0, 129),
            (500_000, 4096),
            (1_999_999, 1),
            (1_998_000, 2000),
        ];

        let mut reader =
            AttachmentReader::new(std::io::Cursor::new(&out), info).expect("info is v2");
        for (pos, read_len) in seeks {
            reader.seek_to(pos).unwrap();
            let mut buf = vec![0u8; read_len as usize];
            reader.read_exact(&mut buf).unwrap();
            assert_eq!(
                buf,
                input[pos as usize..pos as usize + buf.len()],
                "seek to {pos} must decrypt to the plaintext there"
            );
        }
    }
}
