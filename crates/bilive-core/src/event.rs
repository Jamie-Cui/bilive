// Copyright (C) 2026 Jamie Cui
// Author: Jamie Cui
// SPDX-License-Identifier: GPL-3.0-or-later

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum Event {
    Connection(ConnectionStatus),
    DanmuRaw { payload: String },
    Error { message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionStatus {
    Connecting,
    Connected,
    Disconnected,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn serializes_connection_events_with_snake_case_tags() {
        let value = serde_json::to_value(Event::Connection(ConnectionStatus::Connected)).unwrap();

        assert_eq!(
            value,
            json!({
                "type": "connection",
                "payload": "connected",
            })
        );
    }

    #[test]
    fn serializes_danmu_and_error_events() {
        assert_eq!(
            serde_json::to_value(Event::DanmuRaw {
                payload: "hello".to_string(),
            })
            .unwrap(),
            json!({
                "type": "danmu_raw",
                "payload": {
                    "payload": "hello",
                },
            })
        );

        assert_eq!(
            serde_json::to_value(Event::Error {
                message: "bad".to_string(),
            })
            .unwrap(),
            json!({
                "type": "error",
                "payload": {
                    "message": "bad",
                },
            })
        );
    }

    #[test]
    fn deserializes_connection_status_values() {
        let event: Event = serde_json::from_value(json!({
            "type": "connection",
            "payload": "disconnected",
        }))
        .unwrap();

        match event {
            Event::Connection(ConnectionStatus::Disconnected) => {}
            other => panic!("unexpected event: {other:?}"),
        }
    }
}
