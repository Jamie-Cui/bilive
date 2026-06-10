// Copyright (C) 2026 Jamie Cui
// Author: Jamie Cui
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::model::{
    LineKind, OverlayCommand, OverlayEvent, OverlayMessage, chat_line_from_payload, now_millis,
};
use anyhow::{Context, bail};
use bilive_core::{ConnectionStatus, Event};
use futures_util::StreamExt;
use serde::Deserialize;
use serde_json::json;
use std::{collections::HashSet, time::Duration};
use tokio::{sync::mpsc, time};
use tokio_tungstenite::{connect_async, tungstenite::Message as WsMessage};

const RECONNECT_SECS: u64 = 2;

#[derive(Debug, Clone)]
pub struct ServiceUrl {
    base: String,
}

impl ServiceUrl {
    pub fn new(value: &str) -> Self {
        let trimmed = value.trim().trim_end_matches('/');
        let base = if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
            trimmed.to_string()
        } else {
            format!("http://{trimmed}")
        };
        Self { base }
    }

    pub fn api_url(&self, path: &str) -> String {
        format!("{}{}", self.base, path)
    }

    pub fn ws_url(&self, path: &str) -> String {
        let base = self
            .base
            .strip_prefix("http://")
            .map(|rest| format!("ws://{rest}"))
            .or_else(|| {
                self.base
                    .strip_prefix("https://")
                    .map(|rest| format!("wss://{rest}"))
            })
            .unwrap_or_else(|| format!("ws://{}", self.base));
        format!("{base}{path}")
    }
}

#[derive(Clone)]
pub struct ApiClient {
    http: reqwest::Client,
    service: ServiceUrl,
}

impl ApiClient {
    pub fn new(service: ServiceUrl) -> Self {
        Self {
            http: reqwest::Client::new(),
            service,
        }
    }

    pub async fn config(&self) -> anyhow::Result<PublicConfig> {
        Ok(self
            .http
            .get(self.service.api_url("/api/config"))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }

    pub async fn connect_danmu(&self, room_id: u64) -> anyhow::Result<ConnectOutcome> {
        let response = self
            .http
            .post(self.service.api_url("/api/danmu/connect"))
            .json(&json!({ "room_id": room_id }))
            .send()
            .await?;

        match response.status() {
            reqwest::StatusCode::ACCEPTED => Ok(ConnectOutcome::Started),
            reqwest::StatusCode::CONFLICT => Ok(ConnectOutcome::AlreadyConnected),
            _ => {
                let status = response.status();
                let body = response.text().await.unwrap_or_default();
                bail!("HTTP {status}: {}", body.trim())
            }
        }
    }

    pub async fn danmu_messages(&self, room_id: u64) -> anyhow::Result<DanmuMessagesResponse> {
        Ok(self
            .http
            .get(
                self.service
                    .api_url(&format!("/api/danmu/messages?room_id={room_id}")),
            )
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct PublicConfig {
    pub room_id: u64,
    pub room_title: String,
    pub room_token_available: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DanmuMessagesResponse {
    #[allow(dead_code)]
    pub room_id: u64,
    pub items: Vec<DanmuHistoryEntry>,
    #[allow(dead_code)]
    pub total: usize,
    #[allow(dead_code)]
    pub recent_loaded: usize,
    pub recent_error: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DanmuHistoryEntry {
    pub id: String,
    pub payload: String,
    pub received_at: u64,
    pub received_seq: u64,
    pub sent_at: Option<u64>,
    #[allow(dead_code)]
    pub timeline: Option<String>,
    #[allow(dead_code)]
    pub source: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectOutcome {
    Started,
    AlreadyConnected,
}

pub async fn start_live_messages(
    service: ServiceUrl,
    room_id: Option<u64>,
    no_connect: bool,
    show_system: bool,
    tx: mpsc::UnboundedSender<OverlayEvent>,
    command_rx: mpsc::UnboundedReceiver<OverlayCommand>,
) -> anyhow::Result<()> {
    let api = ApiClient::new(service.clone());
    let config = api.config().await.with_context(|| {
        format!(
            "failed to read {}; start bilive first or pass --url",
            service.api_url("/api/config")
        )
    })?;
    let room_id = room_id.unwrap_or(config.room_id);

    let title = if config.room_title.trim().is_empty() {
        format!("room {room_id}")
    } else {
        config.room_title.clone()
    };
    let _ = tx.send(OverlayEvent::Push(OverlayMessage::system(
        format!("status:startup:{}", now_millis()),
        0,
        format!("bilive-danmu: {title}"),
        LineKind::System,
    )));

    if !config.room_token_available {
        let _ = tx.send(OverlayEvent::Push(OverlayMessage::system(
            format!("status:no-token:{}", now_millis()),
            1,
            "没有可用弹幕 token，请先在 Web UI 登录或刷新弹幕信息",
            LineKind::Error,
        )));
    }

    if !no_connect {
        match api.connect_danmu(room_id).await {
            Ok(ConnectOutcome::Started) => {
                let _ = tx.send(OverlayEvent::Push(OverlayMessage::system(
                    format!("status:connect:{}", now_millis()),
                    2,
                    "已请求连接弹幕",
                    LineKind::System,
                )));
            }
            Ok(ConnectOutcome::AlreadyConnected) => {
                let _ = tx.send(OverlayEvent::Push(OverlayMessage::system(
                    format!("status:already-connected:{}", now_millis()),
                    2,
                    "弹幕已经连接",
                    LineKind::System,
                )));
            }
            Err(error) => {
                let _ = tx.send(OverlayEvent::Push(OverlayMessage::system(
                    format!("status:connect-error:{}", now_millis()),
                    2,
                    format!("连接弹幕失败: {error}"),
                    LineKind::Error,
                )));
            }
        }
    }

    spawn_reload_task(api, room_id, command_rx, tx.clone());
    spawn_ws_task(service.ws_url("/api/events"), show_system, tx);
    Ok(())
}

pub fn spawn_test_messages(tx: mpsc::UnboundedSender<OverlayEvent>) {
    tokio::spawn(async move {
        let samples = [
            ("Jamie", "测试聊天：X11/i3 透明置顶点击穿透"),
            ("bilive", "中文字体 fallback 和时间排序"),
            ("SuperChat", "醒目留言会使用不同颜色"),
            ("status", "按 Ctrl-C 退出 overlay"),
        ];
        let mut index = 0u64;
        loop {
            let now = now_millis();
            let (name, content) = samples[index as usize % samples.len()];
            let kind = if name == "SuperChat" {
                LineKind::SuperChat
            } else {
                LineKind::Chat
            };
            let text = if matches!(kind, LineKind::SuperChat) {
                format!("[SC] [{price}] {name}: {content}", price = "¥30")
            } else {
                format!("{name}: {content}")
            };
            if tx
                .send(OverlayEvent::Push(OverlayMessage {
                    id: format!("test:{index}:{now}"),
                    sequence: index,
                    text,
                    kind,
                    received_at: now,
                }))
                .is_err()
            {
                break;
            }
            index = index.saturating_add(1);
            time::sleep(Duration::from_millis(1100)).await;
        }
    });
}

fn spawn_reload_task(
    api: ApiClient,
    room_id: u64,
    mut command_rx: mpsc::UnboundedReceiver<OverlayCommand>,
    tx: mpsc::UnboundedSender<OverlayEvent>,
) {
    tokio::spawn(async move {
        let mut sequence_base = 0u64;
        while let Some(command) = command_rx.recv().await {
            match command {
                OverlayCommand::Reload => {
                    let now = now_millis();
                    match load_history_messages(&api, room_id, sequence_base).await {
                        Ok(messages) => {
                            sequence_base = sequence_base.saturating_add(messages.len() as u64);
                            let count = messages.len();
                            if tx.send(OverlayEvent::Replace(messages)).is_err() {
                                break;
                            }
                            let _ = tx.send(OverlayEvent::Push(OverlayMessage::system(
                                format!("status:reload:{now}"),
                                sequence_base,
                                format!("已重新加载 {count} 条弹幕"),
                                LineKind::System,
                            )));
                            sequence_base = sequence_base.saturating_add(1);
                        }
                        Err(error) => {
                            let _ = tx.send(OverlayEvent::Push(OverlayMessage::system(
                                format!("status:reload-error:{now}"),
                                sequence_base,
                                format!("重新加载弹幕失败: {error}"),
                                LineKind::Error,
                            )));
                            sequence_base = sequence_base.saturating_add(1);
                        }
                    }
                }
            }
        }
    });
}

async fn load_history_messages(
    api: &ApiClient,
    room_id: u64,
    sequence_base: u64,
) -> anyhow::Result<Vec<OverlayMessage>> {
    let response = api.danmu_messages(room_id).await?;
    if let Some(error) = response.recent_error {
        eprintln!("bilive-danmu: recent danmu history load failed during reload: {error}");
    }

    let mut messages = response
        .items
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| history_entry_message(entry, sequence_base, index))
        .collect::<Vec<_>>();
    messages.sort_by(|left, right| {
        left.received_at
            .cmp(&right.received_at)
            .then_with(|| left.sequence.cmp(&right.sequence))
            .then_with(|| left.id.cmp(&right.id))
    });
    Ok(messages)
}

fn history_entry_message(
    entry: &DanmuHistoryEntry,
    sequence_base: u64,
    index: usize,
) -> Option<OverlayMessage> {
    let received_at = entry.sent_at.unwrap_or(entry.received_at);
    let mut line = chat_line_from_payload(&entry.payload, received_at)?;
    line.id = entry.id.clone();
    line.sequence = sequence_base
        .saturating_add(entry.received_seq)
        .saturating_add(index as u64);
    Some(OverlayMessage::from_line(line))
}

fn spawn_ws_task(ws_url: String, show_system: bool, tx: mpsc::UnboundedSender<OverlayEvent>) {
    tokio::spawn(async move {
        let mut seen = HashSet::new();
        let mut sequence = 0u64;

        loop {
            match connect_async(&ws_url).await {
                Ok((mut socket, _)) => {
                    if show_system {
                        let _ = tx.send(OverlayEvent::Push(OverlayMessage::system(
                            format!("status:ws-connected:{}", now_millis()),
                            sequence,
                            "事件流已连接",
                            LineKind::System,
                        )));
                        sequence = sequence.saturating_add(1);
                    }

                    while let Some(message) = socket.next().await {
                        match message {
                            Ok(WsMessage::Text(text)) => {
                                handle_ws_text(
                                    text.as_ref(),
                                    show_system,
                                    &mut seen,
                                    &mut sequence,
                                    &tx,
                                );
                            }
                            Ok(WsMessage::Close(_)) => break,
                            Ok(_) => {}
                            Err(error) => {
                                let _ = tx.send(OverlayEvent::Push(OverlayMessage::system(
                                    format!("status:ws-error:{}", now_millis()),
                                    sequence,
                                    format!("事件流断开: {error}"),
                                    LineKind::Error,
                                )));
                                sequence = sequence.saturating_add(1);
                                break;
                            }
                        }
                    }
                }
                Err(error) => {
                    let _ = tx.send(OverlayEvent::Push(OverlayMessage::system(
                        format!("status:ws-connect-error:{}", now_millis()),
                        sequence,
                        format!("事件流连接失败: {error}"),
                        LineKind::Error,
                    )));
                    sequence = sequence.saturating_add(1);
                }
            }

            time::sleep(Duration::from_secs(RECONNECT_SECS)).await;
        }
    });
}

fn handle_ws_text(
    text: &str,
    show_system: bool,
    seen: &mut HashSet<String>,
    sequence: &mut u64,
    tx: &mpsc::UnboundedSender<OverlayEvent>,
) {
    match serde_json::from_str::<Event>(text) {
        Ok(Event::Connection(status)) => {
            if show_system {
                let _ = tx.send(OverlayEvent::Push(OverlayMessage::system(
                    format!("status:connection:{:?}:{}", status, now_millis()),
                    *sequence,
                    connection_status_text(status),
                    LineKind::System,
                )));
                *sequence = sequence.saturating_add(1);
            }
        }
        Ok(Event::Error { message }) => {
            let _ = tx.send(OverlayEvent::Push(OverlayMessage::system(
                format!("error:{}:{message}", now_millis()),
                *sequence,
                format!("服务错误: {message}"),
                LineKind::Error,
            )));
            *sequence = sequence.saturating_add(1);
        }
        Ok(Event::DanmuRaw { payload }) => {
            let now = now_millis();
            if let Some(mut line) = chat_line_from_payload(&payload, now) {
                if !seen.insert(line.id.clone()) {
                    return;
                }
                line.sequence = *sequence;
                *sequence = sequence.saturating_add(1);
                let _ = tx.send(OverlayEvent::Push(OverlayMessage::from_line(line)));
            }
        }
        Err(error) => {
            let _ = tx.send(OverlayEvent::Push(OverlayMessage::system(
                format!("status:parse-error:{}", now_millis()),
                *sequence,
                format!("事件解析失败: {error}"),
                LineKind::Error,
            )));
            *sequence = sequence.saturating_add(1);
        }
    }
}

fn connection_status_text(status: ConnectionStatus) -> &'static str {
    match status {
        ConnectionStatus::Connecting => "弹幕连接中",
        ConnectionStatus::Connected => "弹幕已连接",
        ConnectionStatus::Disconnected => "弹幕已断开",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn websocket_url_follows_service_scheme() {
        assert_eq!(
            ServiceUrl::new("http://127.0.0.1:22333").ws_url("/api/events"),
            "ws://127.0.0.1:22333/api/events"
        );
        assert_eq!(
            ServiceUrl::new("https://example.test").ws_url("/api/events"),
            "wss://example.test/api/events"
        );
    }

    #[test]
    fn ignores_non_chat_danmu_events_even_when_system_messages_are_enabled() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut seen = HashSet::new();
        let mut sequence = 0;
        let event = serde_json::to_string(&Event::DanmuRaw {
            payload: r#"{"cmd":"STOP_LIVE_ROOM_LIST","data":{"room_id":1}}"#.to_string(),
        })
        .unwrap();

        handle_ws_text(&event, true, &mut seen, &mut sequence, &tx);

        assert!(rx.try_recv().is_err());
        assert_eq!(sequence, 0);
    }

    #[test]
    fn converts_history_entry_to_overlay_message() {
        let entry = DanmuHistoryEntry {
            id: "history-id".to_string(),
            payload: r#"{"cmd":"DANMU_MSG","info":[[0,1,25,0,1780000000],"你好",[42,"Jamie"]]}"#
                .to_string(),
            received_at: 2_000,
            received_seq: 7,
            sent_at: Some(1_780_000_000_000),
            timeline: None,
            source: "history".to_string(),
        };

        let message = history_entry_message(&entry, 100, 3).unwrap();

        assert_eq!(message.id, "history-id");
        assert_eq!(message.sequence, 110);
        assert_eq!(message.received_at, 1_780_000_000_000);
        assert_eq!(message.text, "Jamie: 你好");
    }
}
