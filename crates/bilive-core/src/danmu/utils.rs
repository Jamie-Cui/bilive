// Copyright (C) 2026 Jamie Cui
// Author: Jamie Cui
// SPDX-License-Identifier: GPL-3.0-or-later

use flate2::read::ZlibDecoder;
use std::io::Read;

pub fn zlib_decode(payload: &[u8]) -> Result<Vec<u8>, std::io::Error> {
    let mut decoder = ZlibDecoder::new(payload);
    let mut decoded = Vec::new();
    decoder.read_to_end(&mut decoded)?;
    Ok(decoded)
}

pub fn brotli_decode(payload: &[u8]) -> Result<Vec<u8>, std::io::Error> {
    let mut decoded = Vec::new();
    let mut decoder = brotli::Decompressor::new(payload, 4096);
    decoder.read_to_end(&mut decoded)?;
    Ok(decoded)
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::{Compression, write::ZlibEncoder};
    use std::io::Write;

    #[test]
    fn decodes_zlib_payloads() {
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(b"hello zlib").unwrap();
        let compressed = encoder.finish().unwrap();

        assert_eq!(zlib_decode(&compressed).unwrap(), b"hello zlib");
    }

    #[test]
    fn returns_error_for_invalid_zlib_payloads() {
        assert!(zlib_decode(b"not a zlib stream").is_err());
    }

    #[test]
    fn decodes_brotli_payloads() {
        let mut compressed = Vec::new();
        {
            let mut writer = brotli::CompressorWriter::new(&mut compressed, 4096, 5, 22);
            writer.write_all(b"hello brotli").unwrap();
        }

        assert_eq!(brotli_decode(&compressed).unwrap(), b"hello brotli");
    }
}
