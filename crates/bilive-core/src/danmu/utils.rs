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
