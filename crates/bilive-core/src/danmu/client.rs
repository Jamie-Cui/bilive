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
