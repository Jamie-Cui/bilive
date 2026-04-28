// Copyright (C) 2026 Jamie Cui
// Author: Jamie Cui
// SPDX-License-Identifier: GPL-3.0-or-later

use bytes::{Buf, BufMut, BytesMut};
use tokio_util::codec::{Decoder, Encoder};

#[derive(Debug, Clone)]
pub struct ProtocolHeader {
    pub total_size: u32,
    pub header_size: u16,
    pub protocol_version: u16,
    pub opcode: u32,
    pub sequence: u32,
}

#[derive(Debug, Clone)]
pub struct Message {
    pub header: ProtocolHeader,
    pub payload: Vec<u8>,
}

pub struct Opcode;

impl Opcode {
    pub const HEARTBEAT: u32 = 2;
    pub const NORMAL: u32 = 5;
    pub const AUTH: u32 = 7;
}

pub struct ProtocolVersion;

impl ProtocolVersion {
    pub const COMPRESSED_ZLIB: u16 = 2;
    pub const COMPRESSED_BROTLI: u16 = 3;
}

impl Message {
    fn new(opcode: u32, sequence: u32, payload: Vec<u8>) -> Self {
        let header_size = 16u16;
        let total_size = header_size as u32 + payload.len() as u32;

        Self {
            header: ProtocolHeader {
                total_size,
                header_size,
                protocol_version: 1,
                opcode,
                sequence,
            },
            payload,
        }
    }

    pub fn auth(sequence: u32, auth_data: Vec<u8>) -> Self {
        Self::new(Opcode::AUTH, sequence, auth_data)
    }

    pub fn heartbeat(sequence: u32) -> Self {
        Self::new(Opcode::HEARTBEAT, sequence, Vec::new())
    }
}

pub struct ProtocolCodec;

impl Decoder for ProtocolCodec {
    type Error = std::io::Error;
    type Item = Message;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        if src.len() < 16 {
            return Ok(None);
        }

        let total_size = u32::from_be_bytes([src[0], src[1], src[2], src[3]]);
        let header_size = u16::from_be_bytes([src[4], src[5]]);
        let protocol_version = u16::from_be_bytes([src[6], src[7]]);
        let opcode = u32::from_be_bytes([src[8], src[9], src[10], src[11]]);
        let sequence = u32::from_be_bytes([src[12], src[13], src[14], src[15]]);

        if total_size > 10_000_000 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "danmu message too large",
            ));
        }

        if header_size < 16 || header_size as u32 > total_size {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "invalid danmu packet header",
            ));
        }

        if src.len() < total_size as usize {
            src.reserve(total_size as usize - src.len());
            return Ok(None);
        }

        src.advance(header_size as usize);
        let payload_size = total_size as usize - header_size as usize;
        let payload = src[..payload_size].to_vec();
        src.advance(payload_size);

        Ok(Some(Message {
            header: ProtocolHeader {
                total_size,
                header_size,
                protocol_version,
                opcode,
                sequence,
            },
            payload,
        }))
    }
}

impl Encoder<Message> for ProtocolCodec {
    type Error = std::io::Error;

    fn encode(&mut self, item: Message, dst: &mut BytesMut) -> Result<(), Self::Error> {
        dst.put_u32(item.header.total_size);
        dst.put_u16(item.header.header_size);
        dst.put_u16(item.header.protocol_version);
        dst.put_u32(item.header.opcode);
        dst.put_u32(item.header.sequence);
        dst.extend_from_slice(&item.payload);
        Ok(())
    }
}

pub fn parse_header(view: &[u8]) -> Option<ProtocolHeader> {
    if view.len() < 16 {
        return None;
    }

    Some(ProtocolHeader {
        total_size: u32::from_be_bytes(view[0..4].try_into().ok()?),
        header_size: u16::from_be_bytes(view[4..6].try_into().ok()?),
        protocol_version: u16::from_be_bytes(view[6..8].try_into().ok()?),
        opcode: u32::from_be_bytes(view[8..12].try_into().ok()?),
        sequence: u32::from_be_bytes(view[12..16].try_into().ok()?),
    })
}
