// Copyright (C) 2026 Jamie Cui
// Author: Jamie Cui
// SPDX-License-Identifier: GPL-3.0-or-later

use super::protocol::{Message, Opcode, ProtocolCodec, ProtocolVersion, parse_header};
use super::utils::{brotli_decode, zlib_decode};
use crate::{ConnectionStatus, Event};
use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use std::time::Duration;
use tokio::{net::TcpStream, sync::broadcast, time::interval};
use tokio_util::codec::Framed;
use tracing::warn;

#[derive(Debug, Clone)]
pub struct DanmuConnectOptions {
    pub host: String,
    pub port: u16,
    pub uid: u64,
    pub room_id: u32,
    pub token: String,
}

impl Default for DanmuConnectOptions {
    fn default() -> Self {
        Self {
            host: "broadcastlv.chat.bilibili.com".to_string(),
            port: 2243,
            uid: 0,
            room_id: 0,
            token: String::new(),
        }
    }
}

#[derive(Clone)]
pub struct DanmuClient {
    events: broadcast::Sender<Event>,
}

#[derive(Serialize)]
struct AuthPayload<'a> {
    uid: u64,
    roomid: u32,
    protover: u32,
    platform: &'a str,
    #[serde(rename = "type")]
    kind: u16,
    key: &'a str,
}

impl DanmuClient {
    pub fn new(events: broadcast::Sender<Event>) -> Self {
        Self { events }
    }

    pub async fn connect(
        &self,
        options: DanmuConnectOptions,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let _ = self
            .events
            .send(Event::Connection(ConnectionStatus::Connecting));

        let addr = format!("{}:{}", options.host, options.port);
        let stream = match TcpStream::connect(&addr).await {
            Ok(stream) => stream,
            Err(error) => {
                self.emit_error(format!("connection failed: {error}"));
                return Err(error.into());
            }
        };

        let _ = self
            .events
            .send(Event::Connection(ConnectionStatus::Connected));

        let mut framed = Framed::new(stream, ProtocolCodec);
        let auth = AuthPayload {
            uid: options.uid,
            roomid: options.room_id,
            protover: 3,
            platform: "web",
            kind: 2,
            key: &options.token,
        };
        let auth_json = serde_json::to_vec(&auth)?;
        framed.send(Message::auth(1, auth_json)).await?;

        let mut sequence = 2u32;
        let mut heartbeat = interval(Duration::from_secs(30));
        heartbeat.tick().await;

        loop {
            tokio::select! {
                Some(result) = framed.next() => {
                    match result {
                        Ok(message) => self.handle_message(message),
                        Err(error) => {
                            self.emit_error(format!("receive error: {error}"));
                            break;
                        }
                    }
                }
                _ = heartbeat.tick() => {
                    let packet = Message::heartbeat(sequence);
                    sequence += 1;

                    if let Err(error) = framed.send(packet).await {
                        self.emit_error(format!("heartbeat error: {error}"));
                        break;
                    }
                }
            }
        }

        let _ = self
            .events
            .send(Event::Connection(ConnectionStatus::Disconnected));
        Ok(())
    }

    fn handle_message(&self, message: Message) {
        if message.header.opcode != Opcode::NORMAL {
            return;
        }

        match message.header.protocol_version {
            ProtocolVersion::COMPRESSED_BROTLI => match brotli_decode(&message.payload) {
                Ok(decoded) => self.depack_sub_packets(&decoded),
                Err(error) => self.emit_error(format!("brotli decompress error: {error}")),
            },
            ProtocolVersion::COMPRESSED_ZLIB => match zlib_decode(&message.payload) {
                Ok(decoded) => self.depack_sub_packets(&decoded),
                Err(error) => self.emit_error(format!("zlib decompress error: {error}")),
            },
            _ => self.emit_payload(&message.payload),
        }
    }

    fn depack_sub_packets(&self, buffer: &[u8]) {
        let mut offset = 0usize;
        while offset < buffer.len() {
            let Some(header) = parse_header(&buffer[offset..]) else {
                break;
            };

            let total_size = header.total_size as usize;
            let header_size = header.header_size as usize;
            if total_size == 0 || header_size > total_size || offset + total_size > buffer.len() {
                warn!("invalid danmu sub-packet size");
                break;
            }

            let body_start = offset + header_size;
            let body_end = offset + total_size;
            self.emit_payload(&buffer[body_start..body_end]);
            offset += total_size;
        }
    }

    fn emit_payload(&self, payload: &[u8]) {
        match String::from_utf8(payload.to_vec()) {
            Ok(payload) => {
                let _ = self.events.send(Event::DanmuRaw { payload });
            }
            Err(error) => self.emit_error(format!("invalid message payload: {error}")),
        }
    }

    fn emit_error(&self, message: impl Into<String>) {
        let _ = self.events.send(Event::Error {
            message: message.into(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::super::protocol::ProtocolHeader;
    use super::*;
    use bytes::BytesMut;
    use flate2::{Compression, write::ZlibEncoder};
    use std::io::Write;
    use tokio::sync::broadcast::{self, error::TryRecvError};
    use tokio_util::codec::Encoder;

    fn client_and_receiver() -> (DanmuClient, broadcast::Receiver<Event>) {
        let (events, receiver) = broadcast::channel(16);
        (DanmuClient::new(events), receiver)
    }

    fn message(opcode: u32, protocol_version: u16, payload: Vec<u8>) -> Message {
        Message {
            header: ProtocolHeader {
                total_size: 16 + payload.len() as u32,
                header_size: 16,
                protocol_version,
                opcode,
                sequence: 1,
            },
            payload,
        }
    }

    fn normal_message(protocol_version: u16, payload: &[u8]) -> Message {
        message(Opcode::NORMAL, protocol_version, payload.to_vec())
    }

    fn packet_bytes(payload: &[u8]) -> Vec<u8> {
        let mut buffer = BytesMut::new();
        ProtocolCodec
            .encode(normal_message(1, payload), &mut buffer)
            .unwrap();
        buffer.to_vec()
    }

    fn zlib_compress(payload: &[u8]) -> Vec<u8> {
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(payload).unwrap();
        encoder.finish().unwrap()
    }

    fn brotli_compress(payload: &[u8]) -> Vec<u8> {
        let mut compressed = Vec::new();
        {
            let mut writer = brotli::CompressorWriter::new(&mut compressed, 4096, 5, 22);
            writer.write_all(payload).unwrap();
        }
        compressed
    }

    #[test]
    fn emits_plain_normal_payloads() {
        let (client, mut receiver) = client_and_receiver();

        client.handle_message(normal_message(1, br#"{"cmd":"DANMU_MSG"}"#));

        match receiver.try_recv().unwrap() {
            Event::DanmuRaw { payload } => assert_eq!(payload, r#"{"cmd":"DANMU_MSG"}"#),
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[test]
    fn ignores_non_normal_messages() {
        let (client, mut receiver) = client_and_receiver();

        client.handle_message(message(Opcode::HEARTBEAT, 1, b"ignored".to_vec()));

        assert!(matches!(receiver.try_recv(), Err(TryRecvError::Empty)));
    }

    #[test]
    fn invalid_utf8_payloads_emit_errors() {
        let (client, mut receiver) = client_and_receiver();

        client.handle_message(normal_message(1, &[0xff, 0xfe]));

        match receiver.try_recv().unwrap() {
            Event::Error { message } => assert!(message.contains("invalid message payload")),
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[test]
    fn depacks_zlib_compressed_sub_packets() {
        let (client, mut receiver) = client_and_receiver();
        let packets = [packet_bytes(b"first"), packet_bytes(b"second")].concat();
        let compressed = zlib_compress(&packets);

        client.handle_message(normal_message(
            ProtocolVersion::COMPRESSED_ZLIB,
            &compressed,
        ));

        match receiver.try_recv().unwrap() {
            Event::DanmuRaw { payload } => assert_eq!(payload, "first"),
            other => panic!("unexpected event: {other:?}"),
        }
        match receiver.try_recv().unwrap() {
            Event::DanmuRaw { payload } => assert_eq!(payload, "second"),
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[test]
    fn depacks_brotli_compressed_sub_packets() {
        let (client, mut receiver) = client_and_receiver();
        let packets = [packet_bytes(b"first"), packet_bytes(b"second")].concat();
        let compressed = brotli_compress(&packets);

        client.handle_message(normal_message(
            ProtocolVersion::COMPRESSED_BROTLI,
            &compressed,
        ));

        match receiver.try_recv().unwrap() {
            Event::DanmuRaw { payload } => assert_eq!(payload, "first"),
            other => panic!("unexpected event: {other:?}"),
        }
        match receiver.try_recv().unwrap() {
            Event::DanmuRaw { payload } => assert_eq!(payload, "second"),
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[test]
    fn invalid_compressed_payloads_emit_errors() {
        let (client, mut receiver) = client_and_receiver();

        client.handle_message(normal_message(
            ProtocolVersion::COMPRESSED_ZLIB,
            b"not zlib",
        ));

        match receiver.try_recv().unwrap() {
            Event::Error { message } => assert!(message.contains("zlib decompress error")),
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[test]
    fn depack_stops_after_invalid_sub_packet_header() {
        let (client, mut receiver) = client_and_receiver();
        let mut packets = packet_bytes(b"valid");
        packets.extend_from_slice(&[0; 16]);

        client.depack_sub_packets(&packets);

        match receiver.try_recv().unwrap() {
            Event::DanmuRaw { payload } => assert_eq!(payload, "valid"),
            other => panic!("unexpected event: {other:?}"),
        }
        assert!(matches!(receiver.try_recv(), Err(TryRecvError::Empty)));
    }
}
