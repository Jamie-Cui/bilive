// Copyright (C) 2026 Jamie Cui
// Author: Jamie Cui
// SPDX-License-Identifier: GPL-3.0-or-later

use serde_json::Value;
use std::{
    mem,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    Chat,
    Mine,
    SuperChat,
    System,
    Error,
}

#[derive(Debug, Clone)]
pub struct DanmuLine {
    pub id: String,
    pub sort_at: u64,
    pub sequence: u64,
    pub name: String,
    pub content: String,
    pub price: Option<String>,
    pub kind: LineKind,
}

#[derive(Debug, Clone)]
pub struct OverlayMessage {
    pub id: String,
    pub sequence: u64,
    pub text: String,
    #[allow(dead_code)]
    pub kind: LineKind,
    pub received_at: u64,
}

#[derive(Debug, Clone)]
pub enum OverlayEvent {
    Push(OverlayMessage),
    Replace(Vec<OverlayMessage>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayCommand {
    Reload,
}

impl OverlayMessage {
    pub fn from_line(line: DanmuLine) -> Self {
        let mut text = String::new();
        if matches!(line.kind, LineKind::SuperChat) {
            text.push_str("[SC] ");
        }
        if let Some(price) = &line.price {
            text.push_str(&format!("[{price}] "));
        }
        text.push_str(&line.name);
        text.push_str(": ");
        text.push_str(&line.content);

        Self {
            id: line.id,
            sequence: line.sequence,
            text,
            kind: line.kind,
            received_at: line.sort_at,
        }
    }

    pub fn system(id: String, sequence: u64, text: impl Into<String>, kind: LineKind) -> Self {
        Self {
            id,
            sequence,
            text: text.into(),
            kind,
            received_at: now_millis(),
        }
    }

    pub fn display_text(&self) -> String {
        format!("[{}] {}", format_clock(self.received_at), self.text)
    }
}

pub fn chat_line_from_payload(payload: &str, received_at: u64) -> Option<DanmuLine> {
    let parsed = serde_json::from_str::<Value>(payload).ok()?;
    let cmd = parsed
        .get("cmd")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if cmd.starts_with("DANMU_MSG") {
        return danmu_line(&parsed, payload, received_at);
    }
    if cmd == "SUPER_CHAT_MESSAGE" {
        return super_chat_line(&parsed, payload, received_at);
    }
    None
}

fn danmu_line(value: &Value, raw: &str, received_at: u64) -> Option<DanmuLine> {
    let info = value.get("info").and_then(Value::as_array)?;
    let meta = info.first().and_then(Value::as_array);
    let user = info.get(2).and_then(Value::as_array);
    let extra = meta.and_then(|meta| extract_danmu_extra(meta));
    let content = info
        .get(1)
        .map(value_to_plain_string)
        .filter(|value| !value.is_empty())
        .or_else(|| {
            extra
                .as_ref()
                .and_then(|extra| extra.get("content"))
                .map(value_to_plain_string)
                .filter(|value| !value.is_empty())
        })?;

    let name = user
        .and_then(|user| user.get(1))
        .map(value_to_plain_string)
        .filter(|value| !value.is_empty())
        .or_else(|| {
            extra
                .as_ref()
                .and_then(|extra| extra.get("uname"))
                .map(value_to_plain_string)
                .filter(|value| !value.is_empty())
        })
        .unwrap_or_else(|| "匿名用户".to_string());

    let sort_at = danmu_sort_at(value).unwrap_or(received_at);
    let mine = extra
        .as_ref()
        .and_then(|extra| extra.get("send_from_me"))
        .and_then(Value::as_bool)
        .unwrap_or(false);

    Some(DanmuLine {
        id: danmu_message_id(value, raw),
        sort_at,
        sequence: 0,
        name: sanitize_text(&name),
        content: sanitize_text(&content),
        price: None,
        kind: if mine { LineKind::Mine } else { LineKind::Chat },
    })
}

fn super_chat_line(value: &Value, raw: &str, received_at: u64) -> Option<DanmuLine> {
    let data = value.get("data")?;
    let content = data
        .get("message")
        .map(value_to_plain_string)
        .filter(|value| !value.is_empty())?;
    let name = data
        .get("user_info")
        .and_then(|value| value.get("uname").or_else(|| value.get("name")))
        .or_else(|| data.get("uname"))
        .map(value_to_plain_string)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "匿名用户".to_string());
    let price = data
        .get("price")
        .map(value_to_plain_string)
        .filter(|value| !value.is_empty())
        .map(|value| format!("¥{value}"));
    let sort_at = super_chat_sort_at(value).unwrap_or(received_at);

    Some(DanmuLine {
        id: danmu_message_id(value, raw),
        sort_at,
        sequence: 0,
        name: sanitize_text(&name),
        content: sanitize_text(&content),
        price,
        kind: LineKind::SuperChat,
    })
}

fn danmu_message_id(value: &Value, raw: &str) -> String {
    let cmd = value.get("cmd").and_then(Value::as_str).unwrap_or_default();
    if cmd.starts_with("DANMU_MSG") {
        let info = value
            .get("info")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let meta = info.first().and_then(Value::as_array);
        let user = info.get(2).and_then(Value::as_array);
        let uid = user
            .and_then(|user| user.first())
            .map(value_to_plain_string)
            .unwrap_or_default();
        let content = info.get(1).map(value_to_plain_string).unwrap_or_default();
        let rnd = meta
            .and_then(|meta| meta.get(4).or_else(|| meta.get(13)))
            .map(value_to_plain_string)
            .unwrap_or_default();
        return format!("danmu:{uid}:{content}:{rnd}");
    }

    if cmd == "SUPER_CHAT_MESSAGE" {
        let data = value.get("data").unwrap_or(&Value::Null);
        let id = data
            .get("id")
            .or_else(|| data.get("message_id"))
            .map(value_to_plain_string)
            .unwrap_or_default();
        let uid = data
            .get("uid")
            .or_else(|| data.get("user_info").and_then(|value| value.get("uid")))
            .map(value_to_plain_string)
            .unwrap_or_default();
        let message = data
            .get("message")
            .map(value_to_plain_string)
            .unwrap_or_default();
        return format!("super_chat:{id}:{uid}:{message}");
    }

    format!("raw:{raw}")
}

fn danmu_sort_at(value: &Value) -> Option<u64> {
    value
        .get("info")
        .and_then(Value::as_array)
        .and_then(|info| {
            info.get(9).and_then(|extra| extra.get("ts")).or_else(|| {
                info.first()
                    .and_then(Value::as_array)
                    .and_then(|meta| meta.get(4).or_else(|| meta.get(13)))
            })
        })
        .and_then(epoch_millis)
}

fn super_chat_sort_at(value: &Value) -> Option<u64> {
    value
        .get("data")
        .and_then(|data| {
            data.get("ts")
                .or_else(|| data.get("start_time"))
                .or_else(|| data.get("time"))
        })
        .and_then(epoch_millis)
}

fn epoch_millis(value: &Value) -> Option<u64> {
    let number = value_as_u64(value)?;
    if number == 0 {
        return None;
    }
    if number >= 100_000_000_000 {
        Some(number)
    } else {
        number.checked_mul(1000)
    }
}

fn value_as_u64(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_str().and_then(|text| text.parse::<u64>().ok()))
}

fn value_to_plain_string(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        Value::Number(value) => value.to_string(),
        Value::Bool(value) => value.to_string(),
        _ => String::new(),
    }
}

fn extract_danmu_extra(meta: &[Value]) -> Option<Value> {
    meta.iter()
        .find_map(|item| item.get("extra").and_then(Value::as_str))
        .and_then(|extra| serde_json::from_str::<Value>(extra).ok())
}

pub fn sanitize_text(value: &str) -> String {
    value
        .chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect()
}

pub fn now_millis() -> u64 {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    u64::try_from(millis).unwrap_or(u64::MAX)
}

pub fn format_clock(millis: u64) -> String {
    let formatted =
        local_tm(millis).map(|tm| format!("{:02}:{:02}:{:02}", tm.tm_hour, tm.tm_min, tm.tm_sec));
    formatted.unwrap_or_else(|| "00:00:00".to_string())
}

fn local_tm(millis: u64) -> Option<libc::tm> {
    let seconds = (millis / 1000) as libc::time_t;
    let mut tm = mem::MaybeUninit::<libc::tm>::uninit();
    unsafe {
        if libc::localtime_r(&seconds, tm.as_mut_ptr()).is_null() {
            None
        } else {
            Some(tm.assume_init())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_danmu_message() {
        let line = chat_line_from_payload(
            r#"{"cmd":"DANMU_MSG","info":[[0,1,25,0,1780000000],"你好",[42,"Jamie"],["7","牌子"]]}"#,
            1,
        )
        .unwrap();

        assert_eq!(line.name, "Jamie");
        assert_eq!(line.content, "你好");
        assert_eq!(line.sort_at, 1_780_000_000_000);
    }

    #[test]
    fn parses_super_chat_message() {
        let line = chat_line_from_payload(
            r#"{"cmd":"SUPER_CHAT_MESSAGE","data":{"id":1,"uid":42,"message":"SC 内容","price":30,"user_info":{"uname":"Jamie"},"ts":1780000000}}"#,
            1,
        )
        .unwrap();

        assert_eq!(line.name, "Jamie");
        assert_eq!(line.content, "SC 内容");
        assert_eq!(line.price.as_deref(), Some("¥30"));
        assert_eq!(line.kind, LineKind::SuperChat);
    }

    #[test]
    fn system_messages_are_timestamped() {
        let message = OverlayMessage::system("id".to_string(), 0, "hello", LineKind::System);

        assert_eq!(message.id, "id");
        assert_eq!(message.sequence, 0);
        assert_eq!(message.text, "hello");
        assert!(message.received_at > 0);
    }
}
