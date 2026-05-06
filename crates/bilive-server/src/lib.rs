// Copyright (C) 2026 Jamie Cui
// Author: Jamie Cui
// SPDX-License-Identifier: GPL-3.0-or-later

use anyhow::{Context, bail};
use axum::{
    Json, Router,
    body::Body,
    extract::{
        Path, Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{StatusCode, Uri, header},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
};
use bilive_core::{
    ConfigStore, Event,
    bili::{BiliClient, BiliError},
    danmu::{DanmuClient, DanmuConnectOptions},
};
use futures_util::{SinkExt, StreamExt};
use qrcode::{QrCode as SvgQrCode, render::svg};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    net::SocketAddr,
    path::PathBuf,
    process::{ExitStatus, Stdio},
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::process::Command;
use tokio::sync::Mutex;
use tokio::{net::TcpListener, sync::broadcast, task::JoinHandle};
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;
use tracing::{info, warn};

const TEST_STREAM_DURATION_SECONDS: u64 = 5;

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub listen: SocketAddr,
    pub web_dir: Option<PathBuf>,
    pub config_path: Option<PathBuf>,
}

#[derive(Clone)]
struct AppState {
    events: broadcast::Sender<Event>,
    danmu_task: Arc<Mutex<Option<JoinHandle<()>>>>,
    danmu_log: Arc<Mutex<DanmuHistory>>,
    bili: BiliClient,
}

#[derive(Debug, Serialize)]
struct HealthResponse {
    status: &'static str,
    version: &'static str,
}

#[derive(Debug, Serialize)]
struct DanmuStatusResponse {
    connected: bool,
}

#[derive(Debug, Deserialize)]
struct ConnectDanmuRequest {
    room_id: Option<u32>,
    token: Option<String>,
    uid: Option<u64>,
    #[serde(default = "default_danmu_host")]
    host: String,
    #[serde(default = "default_danmu_port")]
    port: u16,
}

#[derive(Debug, Serialize)]
struct DanmuMessagesResponse {
    room_id: u64,
    items: Vec<DanmuHistoryEntry>,
    total: usize,
    recent_loaded: usize,
    recent_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct DanmuHistoryEntry {
    id: String,
    payload: String,
    received_at: u64,
    received_seq: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    sent_at: Option<u64>,
    timeline: Option<String>,
    source: &'static str,
}

#[derive(Debug, Default)]
struct DanmuHistory {
    room_id: Option<u64>,
    items: Vec<DanmuHistoryEntry>,
    seen: HashSet<String>,
    next_received_seq: u64,
}

#[derive(Debug, Deserialize)]
struct CookieLoginRequest {
    cookie: String,
}

#[derive(Debug, Deserialize)]
struct QrPollRequest {
    qrcode_key: String,
}

#[derive(Debug, Deserialize)]
struct TitleRequest {
    title: String,
}

#[derive(Debug, Deserialize)]
struct AreaRequest {
    area_id: String,
}

#[derive(Debug, Deserialize)]
struct TestStreamRequest {
    #[serde(default)]
    index: usize,
}

#[derive(Debug, Deserialize)]
struct CommentRequest {
    message: String,
}

#[derive(Debug, Deserialize)]
struct AdminRequest {
    uid: String,
}

#[derive(Debug, Deserialize)]
struct SilentUserRequest {
    uid: String,
    hour: String,
}

#[derive(Debug, Deserialize)]
struct RoomSilentRequest {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default = "default_room_silent_level")]
    level: u64,
    #[serde(default)]
    minute: u64,
}

#[derive(Debug, Deserialize)]
struct KeywordRequest {
    keyword: String,
}

#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    message: String,
}

fn default_danmu_host() -> String {
    "broadcastlv.chat.bilibili.com".to_string()
}

fn default_danmu_port() -> u16 {
    2243
}

fn default_room_silent_level() -> u64 {
    1
}

pub async fn run(config: ServerConfig) -> anyhow::Result<()> {
    let web_dir = config.web_dir.map(resolve_web_dir).transpose()?;
    let store = ConfigStore::load(config.config_path.clone())
        .await
        .context("failed to load config")?;
    let bili = BiliClient::new(store.clone()).context("failed to create bilibili client")?;
    let (events, _) = broadcast::channel(1024);
    let danmu_log = Arc::new(Mutex::new(DanmuHistory::default()));
    spawn_danmu_recorder(events.subscribe(), danmu_log.clone());
    spawn_danmu_notifier(events.subscribe(), store.clone());
    let state = AppState {
        events,
        danmu_task: Arc::new(Mutex::new(None)),
        danmu_log,
        bili,
    };

    let routes = Router::new()
        .route("/api/health", get(health))
        .route("/api/events", get(events_ws))
        .route("/api/config", get(get_config).patch(patch_config))
        .route("/api/auth/status", get(auth_status))
        .route("/api/auth/bootstrap", post(auth_bootstrap))
        .route("/api/auth/cookie", post(auth_cookie))
        .route("/api/auth/logout", post(auth_logout))
        .route("/api/auth/qrcode/generate", post(qrcode_generate))
        .route("/api/auth/qrcode/poll", post(qrcode_poll))
        .route("/api/user/nav", get(user_nav))
        .route("/api/live/room-id", get(room_id))
        .route("/api/live/areas", get(area_list))
        .route("/api/live/danmu-info", get(danmu_info))
        .route("/api/live/version", get(live_version))
        .route("/api/live/title", post(update_title))
        .route("/api/live/area", post(update_area))
        .route("/api/live/start", post(start_live))
        .route("/api/live/test-stream", post(test_stream))
        .route("/api/live/stop", post(stop_live))
        .route("/api/live/comment", post(send_comment))
        .route("/api/live/contribution-rank", get(contribution_rank))
        .route("/api/manager/admins", get(room_admins).post(add_room_admin))
        .route("/api/manager/admins/{uid}", delete(delete_room_admin))
        .route(
            "/api/manager/silent-users",
            get(silent_users).post(add_silent_user),
        )
        .route(
            "/api/manager/silent-users/{uid}",
            delete(delete_silent_user),
        )
        .route("/api/manager/search-users", get(search_users))
        .route(
            "/api/manager/room-silent",
            get(room_silent).post(set_room_silent),
        )
        .route(
            "/api/manager/blocked-words",
            get(blocked_words).post(add_blocked_word),
        )
        .route(
            "/api/manager/blocked-words/delete",
            post(delete_blocked_word),
        )
        .route("/api/danmu/connect", post(connect_danmu))
        .route("/api/danmu/disconnect", post(disconnect_danmu))
        .route("/api/danmu/messages", get(danmu_messages))
        .route("/api/danmu/status", get(danmu_status));

    let app = match &web_dir {
        Some(web_dir) => routes.fallback_service(static_service(web_dir.clone())),
        None => routes.fallback(embedded_static),
    }
    .layer(TraceLayer::new_for_http())
    .with_state(state);

    let listener = TcpListener::bind(config.listen)
        .await
        .with_context(|| format!("failed to bind {}", config.listen))?;

    match &web_dir {
        Some(web_dir) => info!(
            "bilive listening on http://{} with web dir {}",
            config.listen,
            web_dir.display()
        ),
        None => info!(
            "bilive listening on http://{} with embedded web UI",
            config.listen
        ),
    }
    info!("config path: {}", store.path().display());

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("server stopped unexpectedly")
}

fn static_service(web_dir: PathBuf) -> ServeDir<ServeFile> {
    let index = web_dir.join("index.html");
    ServeDir::new(web_dir).fallback(ServeFile::new(index))
}

fn resolve_web_dir(web_dir: PathBuf) -> anyhow::Result<PathBuf> {
    let index = web_dir.join("index.html");
    if !index.is_file() {
        bail!("web dir {} does not contain index.html", web_dir.display());
    }
    std::fs::canonicalize(&web_dir)
        .with_context(|| format!("failed to resolve web dir {}", web_dir.display()))
}

#[derive(Debug, Clone, Copy)]
struct EmbeddedAsset {
    content_type: &'static str,
    body: &'static [u8],
}

async fn embedded_static(uri: Uri) -> Response {
    let asset = embedded_asset(uri.path()).unwrap_or_else(embedded_index_asset);
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, asset.content_type)
        .body(Body::from(asset.body))
        .expect("embedded static response is valid")
}

fn embedded_index_asset() -> EmbeddedAsset {
    EmbeddedAsset {
        content_type: "text/html; charset=utf-8",
        body: include_bytes!("../../../web/index.html"),
    }
}

fn embedded_asset(path: &str) -> Option<EmbeddedAsset> {
    match path.trim_start_matches('/') {
        "" | "index.html" => Some(embedded_index_asset()),
        "app.js" => Some(EmbeddedAsset {
            content_type: "text/javascript; charset=utf-8",
            body: include_bytes!("../../../web/app.js"),
        }),
        "styles.css" => Some(EmbeddedAsset {
            content_type: "text/css; charset=utf-8",
            body: include_bytes!("../../../web/styles.css"),
        }),
        "favicon.svg" => Some(EmbeddedAsset {
            content_type: "image/svg+xml",
            body: include_bytes!("../../../web/favicon.svg"),
        }),
        _ => None,
    }
}

fn spawn_danmu_recorder(
    mut events: broadcast::Receiver<Event>,
    danmu_log: Arc<Mutex<DanmuHistory>>,
) {
    tokio::spawn(async move {
        loop {
            match events.recv().await {
                Ok(Event::DanmuRaw { payload }) => {
                    if let Some(entry) =
                        DanmuHistoryEntry::from_payload(payload, unix_millis(), None, "live")
                    {
                        danmu_log.lock().await.push(entry);
                    }
                }
                Ok(_) => {}
                Err(broadcast::error::RecvError::Lagged(count)) => {
                    warn!("danmu recorder lagged by {count} events");
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}

fn spawn_danmu_notifier(mut events: broadcast::Receiver<Event>, config: ConfigStore) {
    tokio::spawn(async move {
        let mut last_notification = Instant::now() - Duration::from_secs(3600);
        loop {
            match events.recv().await {
                Ok(Event::DanmuRaw { payload }) => {
                    let Some(notification) = danmu_notification_from_payload(&payload) else {
                        continue;
                    };
                    let settings = config.get().await.danmu_notifications;
                    if !settings.enabled
                        || notification.kind == DanmuNotificationKind::Danmu && !settings.danmu
                        || notification.kind == DanmuNotificationKind::SuperChat
                            && !settings.super_chat
                    {
                        continue;
                    }

                    if notification.kind == DanmuNotificationKind::Danmu {
                        let cooldown = Duration::from_secs(settings.cooldown_secs);
                        if !cooldown.is_zero() && last_notification.elapsed() < cooldown {
                            continue;
                        }
                    }

                    match send_desktop_notification(&notification, settings.expire_timeout_ms).await
                    {
                        Ok(status) if status.success() => {
                            last_notification = Instant::now();
                        }
                        Ok(status) => {
                            warn!("desktop notification command exited with status {status}");
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                            warn!("desktop notification command not found: {error}");
                        }
                        Err(error) => {
                            warn!("failed to send desktop notification: {error}");
                        }
                    }
                }
                Ok(_) => {}
                Err(broadcast::error::RecvError::Lagged(count)) => {
                    warn!("danmu notifier lagged by {count} events");
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}

impl DanmuHistory {
    fn reset_for_room(&mut self, room_id: u64) {
        self.room_id = Some(room_id);
        self.items.clear();
        self.seen.clear();
        self.next_received_seq = 0;
    }

    fn ensure_room(&mut self, room_id: u64) {
        if self.room_id != Some(room_id) {
            self.reset_for_room(room_id);
        }
    }

    fn push(&mut self, mut entry: DanmuHistoryEntry) -> bool {
        if !self.seen.insert(entry.id.clone()) {
            return false;
        }

        entry.received_seq = self.next_received_seq;
        self.next_received_seq = self.next_received_seq.saturating_add(1);
        self.items.push(entry);
        self.sort();
        true
    }

    fn extend_recent(&mut self, room_id: u64, value: &Value, received_at: u64) -> usize {
        self.ensure_room(room_id);
        let mut added = 0;
        for entry in history_entries(value, received_at) {
            if self.push(entry) {
                added += 1;
            }
        }
        added
    }

    fn snapshot(&self) -> Vec<DanmuHistoryEntry> {
        self.items.clone()
    }

    fn sort(&mut self) {
        self.items.sort_by(|left, right| {
            let left_at = left.sent_at.unwrap_or(left.received_at);
            let right_at = right.sent_at.unwrap_or(right.received_at);
            left_at
                .cmp(&right_at)
                .then_with(|| left.received_seq.cmp(&right.received_seq))
                .then_with(|| left.received_at.cmp(&right.received_at))
                .then_with(|| left.id.cmp(&right.id))
        });
    }
}

impl DanmuHistoryEntry {
    fn from_payload(
        payload: String,
        received_at: u64,
        timeline: Option<String>,
        source: &'static str,
    ) -> Option<Self> {
        let parsed = serde_json::from_str::<Value>(&payload).ok()?;
        if !is_chat_event(&parsed) {
            return None;
        }

        let id = danmu_entry_id(&parsed).unwrap_or_else(|| format!("raw:{payload}"));
        let sent_at = danmu_sent_at(&parsed);
        Some(Self {
            id,
            payload,
            received_at,
            received_seq: 0,
            sent_at,
            timeline,
            source,
        })
    }
}

fn history_entries(value: &Value, received_at: u64) -> Vec<DanmuHistoryEntry> {
    ["admin", "room"]
        .into_iter()
        .filter_map(|key| value.get(key).and_then(Value::as_array))
        .flat_map(|items| items.iter())
        .filter_map(|item| history_entry(item, received_at))
        .collect()
}

fn history_entry(item: &Value, received_at: u64) -> Option<DanmuHistoryEntry> {
    let text = item.get("text").and_then(Value::as_str)?.trim();
    if text.is_empty() {
        return None;
    }

    let uid = item.get("uid").and_then(value_as_u64).unwrap_or_default();
    let nickname = item
        .get("nickname")
        .and_then(Value::as_str)
        .unwrap_or("匿名用户");
    let color = item
        .get("color")
        .or_else(|| item.get("text_color"))
        .map(value_to_plain_string)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "16777215".to_string());
    let rnd = item
        .get("rnd")
        .map(value_to_plain_string)
        .filter(|value| !value.is_empty())
        .or_else(|| {
            item.get("check_info")
                .and_then(|value| value.get("ts"))
                .map(value_to_plain_string)
                .filter(|value| !value.is_empty())
        })
        .unwrap_or_default();
    let timeline = item
        .get("timeline")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let medal = item
        .get("medal")
        .filter(|value| value.is_array())
        .cloned()
        .unwrap_or_else(|| Value::Array(Vec::new()));
    let payload = json!({
        "cmd": "DANMU_MSG",
        "info": [
            [0, 1, 25, color, rnd],
            text,
            [uid, nickname],
            medal,
            [],
            [],
            0,
            0,
            null,
            { "ts": rnd }
        ],
    })
    .to_string();

    DanmuHistoryEntry::from_payload(payload, received_at, timeline, "history")
}

fn is_chat_event(value: &Value) -> bool {
    let cmd = value.get("cmd").and_then(Value::as_str).unwrap_or_default();
    cmd.starts_with("DANMU_MSG") || cmd == "SUPER_CHAT_MESSAGE"
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DanmuNotificationKind {
    Danmu,
    SuperChat,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DanmuNotification {
    kind: DanmuNotificationKind,
    title: String,
    body: String,
}

fn danmu_notification_from_payload(payload: &str) -> Option<DanmuNotification> {
    let value: Value = serde_json::from_str(payload).ok()?;
    let cmd = value.get("cmd").and_then(Value::as_str)?;
    if cmd.starts_with("DANMU_MSG") {
        return danmu_message_notification(&value);
    }
    if cmd == "SUPER_CHAT_MESSAGE" {
        return super_chat_notification(&value);
    }
    None
}

fn danmu_message_notification(value: &Value) -> Option<DanmuNotification> {
    let info = value.get("info").and_then(Value::as_array)?;
    let text = info.get(1).map(value_to_plain_string).unwrap_or_default();
    if text.is_empty() {
        return None;
    }
    let user = info.get(2).and_then(Value::as_array);
    let name = user
        .and_then(|user| user.get(1))
        .map(value_to_plain_string)
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "用户".to_string());

    Some(DanmuNotification {
        kind: DanmuNotificationKind::Danmu,
        title: format!("bilive - {name}"),
        body: truncate_notification_body(&text),
    })
}

fn super_chat_notification(value: &Value) -> Option<DanmuNotification> {
    let data = value.get("data")?;
    let message = data
        .get("message")
        .map(value_to_plain_string)
        .unwrap_or_default();
    if message.is_empty() {
        return None;
    }
    let name = data
        .get("user_info")
        .and_then(|value| value.get("uname").or_else(|| value.get("name")))
        .or_else(|| data.get("uname"))
        .map(value_to_plain_string)
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "用户".to_string());
    let price = data
        .get("price")
        .map(value_to_plain_string)
        .filter(|price| !price.is_empty());
    let title = price
        .map(|price| format!("bilive - 醒目留言 {price}元"))
        .unwrap_or_else(|| "bilive - 醒目留言".to_string());

    Some(DanmuNotification {
        kind: DanmuNotificationKind::SuperChat,
        title,
        body: truncate_notification_body(&format!("{name}: {message}")),
    })
}

fn truncate_notification_body(value: &str) -> String {
    const LIMIT: usize = 160;
    let mut body: String = value.chars().take(LIMIT).collect();
    if value.chars().count() > LIMIT {
        body.push('…');
    }
    body
}

async fn send_desktop_notification(
    notification: &DanmuNotification,
    expire_timeout_ms: u64,
) -> std::io::Result<ExitStatus> {
    #[cfg(target_os = "linux")]
    {
        let mut command = Command::new("notify-send");
        command.arg("--app-name=bilive");
        if expire_timeout_ms > 0 {
            command.arg(format!("--expire-time={expire_timeout_ms}"));
        }
        return command
            .arg(&notification.title)
            .arg(&notification.body)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await;
    }

    #[cfg(target_os = "macos")]
    {
        let _ = expire_timeout_ms;
        return Command::new("osascript")
            .arg("-e")
            .arg(format!(
                "display notification {} with title {}",
                applescript_string(&notification.body),
                applescript_string(&notification.title)
            ))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await;
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = notification;
        let _ = expire_timeout_ms;
        Command::new("false").status().await
    }
}

#[cfg(target_os = "macos")]
fn applescript_string(value: &str) -> String {
    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

fn danmu_entry_id(value: &Value) -> Option<String> {
    let cmd = value.get("cmd").and_then(Value::as_str)?;
    if cmd.starts_with("DANMU_MSG") {
        let info = value.get("info").and_then(Value::as_array)?;
        let content = info.get(1).map(value_to_plain_string).unwrap_or_default();
        let uid = info
            .get(2)
            .and_then(Value::as_array)
            .and_then(|user| user.first())
            .map(value_to_plain_string)
            .unwrap_or_default();
        let rnd = info
            .first()
            .and_then(Value::as_array)
            .and_then(|meta| meta.get(4).or_else(|| meta.get(13)))
            .map(value_to_plain_string)
            .unwrap_or_default();
        if uid.is_empty() && content.is_empty() {
            return None;
        }
        return Some(format!("danmu:{uid}:{content}:{rnd}"));
    }

    if cmd == "SUPER_CHAT_MESSAGE" {
        let data = value.get("data")?;
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
        if id.is_empty() && uid.is_empty() && message.is_empty() {
            return None;
        }
        return Some(format!("super_chat:{id}:{uid}:{message}"));
    }

    None
}

fn danmu_sent_at(value: &Value) -> Option<u64> {
    let cmd = value.get("cmd").and_then(Value::as_str)?;
    if cmd.starts_with("DANMU_MSG") {
        return value
            .get("info")
            .and_then(Value::as_array)
            .and_then(|info| {
                info.get(9).and_then(|extra| extra.get("ts")).or_else(|| {
                    info.first()
                        .and_then(Value::as_array)
                        .and_then(|meta| meta.get(4).or_else(|| meta.get(13)))
                })
            })
            .and_then(epoch_millis);
    }

    if cmd == "SUPER_CHAT_MESSAGE" {
        return value
            .get("data")
            .and_then(|data| {
                data.get("ts")
                    .or_else(|| data.get("start_time"))
                    .or_else(|| data.get("time"))
            })
            .and_then(epoch_millis);
    }

    None
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

fn unix_millis() -> u64 {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    u64::try_from(millis).unwrap_or(u64::MAX)
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
    })
}

async fn get_config(State(state): State<AppState>) -> Json<Value> {
    Json(public_config(state.bili.config().await))
}

async fn patch_config(
    State(state): State<AppState>,
    Json(patch): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let config = state.bili.patch_config(patch).await?;
    Ok(Json(public_config(config)))
}

async fn auth_status(State(state): State<AppState>) -> Json<Value> {
    let status = state.bili.login_status().await;
    Json(json!({
        "authenticated": status.authenticated,
        "config": public_config(status.config),
        "config_path": status.config_path,
    }))
}

async fn auth_bootstrap(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let result = state.bili.bootstrap().await?;
    Ok(Json(json!({ "config": public_config(result.config) })))
}

async fn auth_cookie(
    State(state): State<AppState>,
    Json(request): Json<CookieLoginRequest>,
) -> Result<Json<Value>, ApiError> {
    let result = state.bili.set_cookie_login(&request.cookie).await?;
    Ok(Json(json!({ "config": public_config(result.config) })))
}

async fn auth_logout(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let config = state.bili.logout().await?;
    Ok(Json(public_config(config)))
}

async fn qrcode_generate(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let qr = state.bili.qrcode_generate().await?;
    let svg = render_qr_svg(&qr.url)?;
    Ok(Json(json!({
        "url": qr.url,
        "qrcode_key": qr.qrcode_key,
        "svg": svg,
    })))
}

async fn qrcode_poll(
    State(state): State<AppState>,
    Json(request): Json<QrPollRequest>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(state.bili.qrcode_poll(&request.qrcode_key).await?))
}

async fn user_nav(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    Ok(Json(
        serde_json::to_value(state.bili.user_info().await?).unwrap_or(Value::Null),
    ))
}

async fn room_id(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    let uid = params
        .get("uid")
        .and_then(|value| value.parse::<u64>().ok())
        .ok_or_else(|| ApiError::bad_request("missing uid"))?;
    Ok(Json(
        serde_json::to_value(state.bili.room_id_by_uid(uid).await?).unwrap_or(Value::Null),
    ))
}

async fn area_list(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    Ok(Json(state.bili.area_list().await?))
}

async fn danmu_info(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    let config = state.bili.config().await;
    let room_id = params
        .get("room_id")
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(config.room_id);
    Ok(Json(state.bili.danmu_info(room_id).await?))
}

async fn live_version(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    Ok(Json(state.bili.live_version().await?))
}

async fn update_title(
    State(state): State<AppState>,
    Json(request): Json<TitleRequest>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(state.bili.update_room_title(request.title).await?))
}

async fn update_area(
    State(state): State<AppState>,
    Json(request): Json<AreaRequest>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(state.bili.update_room_area(request.area_id).await?))
}

async fn start_live(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let mut response = state.bili.start_live().await?;
    if response.get("code").and_then(Value::as_i64) == Some(0) {
        let room_id = state.bili.config().await.room_id;
        state.danmu_log.lock().await.reset_for_room(room_id);
    }
    if requires_face_auth(&response) {
        let config = state.bili.config().await;
        if config.uid != 0 {
            let url = face_auth_url(config.uid);
            let svg = render_qr_svg(&url)?;
            if let Some(object) = response.as_object_mut() {
                object.insert("face_auth".to_string(), json!({ "url": url, "svg": svg }));
            }
        }
    }
    Ok(Json(response))
}

async fn test_stream(
    State(state): State<AppState>,
    Json(request): Json<TestStreamRequest>,
) -> Result<Json<Value>, ApiError> {
    let config = state.bili.config().await;
    if !config.is_open_live {
        return Err(ApiError::bad_request("请先开播后再测试推流"));
    }

    let stream = config
        .streams
        .get(request.index)
        .ok_or_else(|| ApiError::bad_request("没有可用的推流凭证"))?;
    let url = stream_url(&stream.address, &stream.key);
    if url.is_empty() {
        return Err(ApiError::bad_request("推流地址为空"));
    }

    let room_info = state.bili.room_info(config.room_id).await?;
    if room_info.get("live_status").and_then(Value::as_i64) != Some(1) {
        return Err(ApiError::bad_request(
            "B 站公开状态未开播，请先点击开始直播重新获取推流凭证",
        ));
    }

    let ffmpeg = std::env::var("BILIVE_FFMPEG").unwrap_or_else(|_| "ffmpeg".to_string());
    let started = Instant::now();
    let child = Command::new(ffmpeg)
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-nostdin",
            "-re",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=1280x720:rate=30",
            "-f",
            "lavfi",
            "-i",
            "anullsrc=channel_layout=stereo:sample_rate=44100",
            "-t",
            "5",
            "-c:v",
            "libx264",
            "-preset",
            "veryfast",
            "-tune",
            "zerolatency",
            "-pix_fmt",
            "yuv420p",
            "-b:v",
            "2500k",
            "-maxrate",
            "2500k",
            "-bufsize",
            "5000k",
            "-g",
            "60",
            "-c:a",
            "aac",
            "-b:a",
            "128k",
            "-ar",
            "44100",
            "-flvflags",
            "no_duration_filesize",
            "-f",
            "flv",
        ])
        .arg(url)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| ApiError::bad_request(format!("启动 ffmpeg 失败: {error}")))?;

    let output =
        match tokio::time::timeout(Duration::from_secs(20), child.wait_with_output()).await {
            Ok(result) => result
                .map_err(|error| ApiError::bad_request(format!("等待 ffmpeg 失败: {error}")))?,
            Err(_) => return Err(ApiError::bad_request("测试推流超时")),
        };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if is_rtmp_close_warning(&stderr, started.elapsed()) {
            return Ok(Json(json!({
                "ok": true,
                "message": "测试推流完成，ffmpeg 收尾阶段收到 RTMP 断开警告",
                "duration_seconds": TEST_STREAM_DURATION_SECONDS,
                "stream_type": stream.kind,
                "warning": sanitize_ffmpeg_error(&stderr, &stream.key),
            })));
        }

        return Err(ApiError::bad_request(format!(
            "测试推流失败，ffmpeg 退出状态: {}; {}",
            output.status,
            sanitize_ffmpeg_error(&stderr, &stream.key)
        )));
    }

    Ok(Json(json!({
        "ok": true,
        "message": "测试推流完成",
        "duration_seconds": TEST_STREAM_DURATION_SECONDS,
        "stream_type": stream.kind,
    })))
}

async fn stop_live(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    Ok(Json(state.bili.stop_live().await?))
}

async fn send_comment(
    State(state): State<AppState>,
    Json(request): Json<CommentRequest>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(state.bili.send_comment(request.message).await?))
}

async fn contribution_rank(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    Ok(Json(state.bili.contribution_rank().await?))
}

async fn danmu_messages(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<DanmuMessagesResponse>, ApiError> {
    let config = state.bili.config().await;
    let room_id = params
        .get("room_id")
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(config.room_id);
    if room_id == 0 {
        return Err(ApiError::bad_request("missing room_id"));
    }

    {
        let mut log = state.danmu_log.lock().await;
        log.ensure_room(room_id);
    }

    let include_recent = params
        .get("include_recent")
        .is_none_or(|value| value != "0" && value != "false");
    let mut recent_loaded = 0;
    let mut recent_error = None;

    if include_recent {
        match state.bili.danmu_history(room_id).await {
            Ok(value) => {
                recent_loaded =
                    state
                        .danmu_log
                        .lock()
                        .await
                        .extend_recent(room_id, &value, unix_millis());
            }
            Err(error) => {
                warn!("failed to load recent danmu history: {error}");
                recent_error = Some(error.to_string());
            }
        }
    }

    let items = state.danmu_log.lock().await.snapshot();
    Ok(Json(DanmuMessagesResponse {
        room_id,
        total: items.len(),
        items,
        recent_loaded,
        recent_error,
    }))
}

async fn room_admins(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    let page = params
        .get("page")
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(1);
    Ok(Json(state.bili.room_admins(page).await?))
}

async fn add_room_admin(
    State(state): State<AppState>,
    Json(request): Json<AdminRequest>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(state.bili.add_room_admin(request.uid).await?))
}

async fn delete_room_admin(
    State(state): State<AppState>,
    Path(uid): Path<String>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(state.bili.delete_room_admin(uid).await?))
}

async fn silent_users(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    let page = params
        .get("page")
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(1);
    Ok(Json(state.bili.silent_users(page).await?))
}

async fn search_users(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, ApiError> {
    let search = params.get("search").cloned().unwrap_or_default();
    Ok(Json(state.bili.search_users(search).await?))
}

async fn add_silent_user(
    State(state): State<AppState>,
    Json(request): Json<SilentUserRequest>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(
        state
            .bili
            .add_silent_user(request.uid, request.hour)
            .await?,
    ))
}

async fn delete_silent_user(
    State(state): State<AppState>,
    Path(uid): Path<String>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(state.bili.delete_silent_user(uid).await?))
}

async fn room_silent(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    Ok(Json(state.bili.room_silent().await?))
}

async fn set_room_silent(
    State(state): State<AppState>,
    Json(request): Json<RoomSilentRequest>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(
        state
            .bili
            .set_room_silent(request.kind, request.level, request.minute)
            .await?,
    ))
}

async fn blocked_words(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    Ok(Json(state.bili.blocked_words().await?))
}

async fn add_blocked_word(
    State(state): State<AppState>,
    Json(request): Json<KeywordRequest>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(state.bili.add_blocked_word(request.keyword).await?))
}

async fn delete_blocked_word(
    State(state): State<AppState>,
    Json(request): Json<KeywordRequest>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(state.bili.delete_blocked_word(request.keyword).await?))
}

async fn events_ws(State(state): State<AppState>, upgrade: WebSocketUpgrade) -> impl IntoResponse {
    upgrade.on_upgrade(move |socket| handle_events_socket(state, socket))
}

async fn handle_events_socket(state: AppState, socket: WebSocket) {
    let mut events = state.events.subscribe();
    let (mut sender, mut receiver) = socket.split();

    let send_task = tokio::spawn(async move {
        while let Ok(event) = events.recv().await {
            let Ok(payload) = serde_json::to_string(&event) else {
                continue;
            };

            if sender.send(Message::Text(payload.into())).await.is_err() {
                break;
            }
        }
    });

    while let Some(message) = receiver.next().await {
        match message {
            Ok(Message::Close(_)) => break,
            Ok(_) => {}
            Err(error) => {
                warn!("event websocket receive error: {error}");
                break;
            }
        }
    }

    send_task.abort();
}

async fn connect_danmu(
    State(state): State<AppState>,
    Json(request): Json<ConnectDanmuRequest>,
) -> Result<StatusCode, ApiError> {
    let mut task = state.danmu_task.lock().await;
    if task.as_ref().is_some_and(|handle| !handle.is_finished()) {
        return Err(ApiError {
            status: StatusCode::CONFLICT,
            message: "danmu is already connected".to_string(),
        });
    }

    let config = state.bili.config().await;
    let room_id = request
        .room_id
        .or_else(|| u32::try_from(config.room_id).ok())
        .ok_or_else(|| ApiError::bad_request("missing room_id"))?;
    state.danmu_log.lock().await.ensure_room(u64::from(room_id));
    let uid = request.uid.unwrap_or(config.uid);
    let token = request
        .token
        .filter(|token| !token.is_empty())
        .unwrap_or(config.room_token);
    if token.is_empty() {
        return Err(ApiError::bad_request("missing danmu token"));
    }

    let client = DanmuClient::new(state.events.clone());
    let options = DanmuConnectOptions {
        host: request.host,
        port: request.port,
        uid,
        room_id,
        token,
    };

    *task = Some(tokio::spawn(async move {
        if let Err(error) = client.connect(options).await {
            warn!("danmu connection stopped: {error}");
        }
    }));

    Ok(StatusCode::ACCEPTED)
}

async fn disconnect_danmu(State(state): State<AppState>) -> StatusCode {
    let mut task = state.danmu_task.lock().await;
    if let Some(handle) = task.take() {
        handle.abort();
    }

    let _ = state.events.send(Event::Connection(
        bilive_core::ConnectionStatus::Disconnected,
    ));
    StatusCode::NO_CONTENT
}

async fn danmu_status(State(state): State<AppState>) -> Json<DanmuStatusResponse> {
    let task = state.danmu_task.lock().await;
    Json(DanmuStatusResponse {
        connected: task.as_ref().is_some_and(|handle| !handle.is_finished()),
    })
}

fn public_config(config: bilive_core::AppConfig) -> Value {
    json!({
        "area_list": config.area_list,
        "theme": config.theme,
        "uid": config.uid,
        "avatar": config.avatar,
        "username": config.username,
        "room_id": config.room_id,
        "room_title": config.room_title,
        "category_id": config.category_id,
        "area_id": config.area_id,
        "room_token_available": !config.room_token.is_empty(),
        "is_open_live": config.is_open_live,
        "streams": config.streams,
        "danmu_notifications": config.danmu_notifications,
    })
}

fn render_qr_svg(value: &str) -> Result<String, ApiError> {
    Ok(SvgQrCode::new(value.as_bytes())
        .map_err(|_| ApiError::bad_request("failed to render QR code"))?
        .render::<svg::Color>()
        .min_dimensions(220, 220)
        .dark_color(svg::Color("#111827"))
        .light_color(svg::Color("#ffffff"))
        .build())
}

fn requires_face_auth(response: &Value) -> bool {
    response.get("code").and_then(Value::as_i64) == Some(60024)
        || response_text_contains(response, "身份验证")
        || response_text_contains(response, "人脸")
}

fn response_text_contains(response: &Value, needle: &str) -> bool {
    ["message", "msg"].iter().any(|key| {
        response
            .get(*key)
            .and_then(Value::as_str)
            .is_some_and(|text| text.contains(needle))
    })
}

fn face_auth_url(uid: u64) -> String {
    format!(
        "https://www.bilibili.com/blackboard/live/face-auth-middle.html?source_event=400&mid={uid}"
    )
}

fn stream_url(address: &str, key: &str) -> String {
    if address.is_empty() || key.is_empty() {
        return format!("{address}{key}");
    }
    if key.starts_with('?') || key.starts_with('&') {
        return format!("{address}{key}");
    }
    format!(
        "{}{slash}{key}",
        address,
        slash = if address.ends_with('/') { "" } else { "/" }
    )
}

fn sanitize_ffmpeg_error(stderr: &str, stream_key: &str) -> String {
    let text = stderr.trim();
    if text.is_empty() {
        return "ffmpeg 没有返回错误详情".to_string();
    }
    let sanitized = text.replace(stream_key, "<stream-key>");
    let sanitized = sanitized
        .lines()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("\n");
    sanitized.chars().take(1200).collect()
}

fn is_rtmp_close_warning(stderr: &str, elapsed: Duration) -> bool {
    elapsed >= Duration::from_secs(TEST_STREAM_DURATION_SECONDS.saturating_sub(1))
        && stderr.contains("Broken pipe")
        && (stderr.contains("Error writing trailer") || stderr.contains("Error closing file"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_assets_include_static_ui_files() {
        let index = embedded_asset("/").unwrap();
        assert_eq!(index.content_type, "text/html; charset=utf-8");
        assert!(
            std::str::from_utf8(index.body)
                .unwrap()
                .contains(r#"<script type="module" src="/app.js"></script>"#)
        );

        assert_eq!(
            embedded_asset("/app.js").unwrap().content_type,
            "text/javascript; charset=utf-8"
        );
        assert_eq!(
            embedded_asset("/styles.css").unwrap().content_type,
            "text/css; charset=utf-8"
        );
        assert_eq!(
            embedded_asset("/favicon.svg").unwrap().content_type,
            "image/svg+xml"
        );
        assert!(embedded_asset("/missing.txt").is_none());
    }

    #[test]
    fn keeps_bilibili_query_style_stream_url() {
        let url = stream_url(
            "rtmp://live-push.bilivideo.com/live-bvc/",
            "?streamname=live_1_2&key=secret&schedule=rtmp&pflag=2",
        );

        assert_eq!(
            url,
            "rtmp://live-push.bilivideo.com/live-bvc/?streamname=live_1_2&key=secret&schedule=rtmp&pflag=2"
        );
    }

    #[test]
    fn treats_late_trailer_broken_pipe_as_close_warning() {
        let stderr = "[out#0/flv] Error writing trailer: Broken pipe\n[out#0/flv] Error closing file: Broken pipe";

        assert!(is_rtmp_close_warning(
            stderr,
            Duration::from_secs(TEST_STREAM_DURATION_SECONDS)
        ));
    }

    #[test]
    fn rejects_immediate_broken_pipe_as_close_warning() {
        let stderr = "[out#0/flv] Error writing trailer: Broken pipe";

        assert!(!is_rtmp_close_warning(stderr, Duration::from_secs(1)));
    }

    #[test]
    fn stream_url_joins_plain_keys_with_one_slash() {
        assert_eq!(
            stream_url("rtmp://live-push.example/live", "stream-key"),
            "rtmp://live-push.example/live/stream-key"
        );
        assert_eq!(
            stream_url("rtmp://live-push.example/live/", "stream-key"),
            "rtmp://live-push.example/live/stream-key"
        );
        assert_eq!(stream_url("", "stream-key"), "stream-key");
        assert_eq!(
            stream_url("rtmp://live-push.example/live", ""),
            "rtmp://live-push.example/live"
        );
    }

    #[test]
    fn public_config_omits_auth_secrets_and_exposes_token_availability() {
        let config = bilive_core::AppConfig {
            cookies: vec![bilive_core::AppCookie {
                name: "SESSDATA".to_string(),
                value: "cookie-secret".to_string(),
            }],
            csrf: Some("csrf-secret".to_string()),
            room_token: "room-token-secret".to_string(),
            uid: 100,
            room_id: 200,
            username: Some("tester".to_string()),
            streams: vec![bilive_core::StreamCredential {
                kind: "rtmp-1".to_string(),
                address: "rtmp://example.test/live".to_string(),
                key: "stream-key".to_string(),
            }],
            ..Default::default()
        };

        let value = public_config(config);

        assert_eq!(value["uid"], 100);
        assert_eq!(value["room_id"], 200);
        assert_eq!(value["username"], "tester");
        assert_eq!(value["room_token_available"], true);
        assert_eq!(value["danmu_notifications"]["enabled"], false);
        assert_eq!(value["danmu_notifications"]["expire_timeout_ms"], 0);
        assert!(value.get("cookies").is_none());
        assert!(value.get("csrf").is_none());
        assert!(value.get("room_token").is_none());
        assert!(!value.to_string().contains("room-token-secret"));
    }

    #[test]
    fn extracts_plain_danmu_desktop_notification() {
        let notification = danmu_notification_from_payload(
            r#"{"cmd":"DANMU_MSG","info":[[0,1,25,0,123],"你好",[42,"Jamie"]]}"#,
        )
        .unwrap();

        assert_eq!(notification.kind, DanmuNotificationKind::Danmu);
        assert_eq!(notification.title, "bilive - Jamie");
        assert_eq!(notification.body, "你好");
    }

    #[test]
    fn extracts_super_chat_desktop_notification() {
        let notification = danmu_notification_from_payload(
            r#"{"cmd":"SUPER_CHAT_MESSAGE","data":{"message":"SC 内容","price":30,"user_info":{"uname":"Jamie"}}}"#,
        )
        .unwrap();

        assert_eq!(notification.kind, DanmuNotificationKind::SuperChat);
        assert_eq!(notification.title, "bilive - 醒目留言 30元");
        assert_eq!(notification.body, "Jamie: SC 内容");
    }

    #[test]
    fn detects_face_auth_from_code_message_or_msg() {
        assert!(requires_face_auth(&json!({ "code": 60024 })));
        assert!(requires_face_auth(&json!({ "message": "需要身份验证" })));
        assert!(requires_face_auth(&json!({ "msg": "请完成人脸认证" })));
        assert!(!requires_face_auth(&json!({ "code": 0, "message": "ok" })));
    }

    #[test]
    fn face_auth_url_embeds_uid() {
        assert_eq!(
            face_auth_url(123),
            "https://www.bilibili.com/blackboard/live/face-auth-middle.html?source_event=400&mid=123"
        );
    }

    #[test]
    fn render_qr_svg_returns_svg_markup() {
        let svg = render_qr_svg("https://example.test/login").unwrap();

        assert!(svg.contains("<svg"));
        assert!(svg.contains("#111827"));
        assert!(svg.contains("#ffffff"));
    }

    #[test]
    fn converts_recent_history_items_to_danmu_payloads() {
        let value = json!({
            "room": [{
                "text": "你好",
                "uid": 42,
                "nickname": "tester",
                "timeline": "2026-04-30 12:00:00",
                "rnd": 1780000000u64,
                "color": 5816798u64,
                "medal": [12, "牌子"]
            }]
        });

        let entries = history_entries(&value, 1000);

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].source, "history");
        assert_eq!(entries[0].timeline.as_deref(), Some("2026-04-30 12:00:00"));
        assert_eq!(entries[0].id, "danmu:42:你好:1780000000");

        let payload = serde_json::from_str::<Value>(&entries[0].payload).unwrap();
        assert_eq!(payload["cmd"], "DANMU_MSG");
        assert_eq!(payload["info"][1], "你好");
        assert_eq!(payload["info"][2][1], "tester");
        assert_eq!(payload["info"][9]["ts"], "1780000000");
        assert_eq!(entries[0].sent_at, Some(1780000000000));
    }

    #[test]
    fn danmu_history_keeps_unique_entries_sorted_by_message_time() {
        let early = json!({
            "cmd": "DANMU_MSG",
            "info": [[0, 1, 25, 16777215, 900], "early", [1, "a"], [], [], [], 0, 0, null, { "ts": 100 }]
        })
        .to_string();
        let late = json!({
            "cmd": "DANMU_MSG",
            "info": [[0, 1, 25, 16777215, 100], "late", [2, "b"], [], [], [], 0, 0, null, { "ts": 200 }]
        })
        .to_string();

        let mut history = DanmuHistory::default();
        assert!(history.push(DanmuHistoryEntry::from_payload(late, 1, None, "live").unwrap()));
        assert!(
            history.push(DanmuHistoryEntry::from_payload(early.clone(), 2, None, "live").unwrap())
        );
        assert!(!history.push(DanmuHistoryEntry::from_payload(early, 3, None, "live").unwrap()));

        let items = history.snapshot();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].id, "danmu:1:early:900");
        assert_eq!(items[0].sent_at, Some(100000));
        assert_eq!(items[1].id, "danmu:2:late:100");
        assert_eq!(items[1].sent_at, Some(200000));
    }

    #[test]
    fn danmu_history_preserves_arrival_order_with_same_message_time() {
        let first = json!({
            "cmd": "DANMU_MSG",
            "info": [[0, 1, 25, 16777215, 300], "z later id", [9, "a"], [], [], [], 0, 0, null, { "ts": 100 }]
        })
        .to_string();
        let second = json!({
            "cmd": "DANMU_MSG",
            "info": [[0, 1, 25, 16777215, 100], "a earlier id", [1, "b"], [], [], [], 0, 0, null, { "ts": 100 }]
        })
        .to_string();

        let mut history = DanmuHistory::default();
        assert!(history.push(DanmuHistoryEntry::from_payload(first, 20, None, "history").unwrap()));
        assert!(
            history.push(DanmuHistoryEntry::from_payload(second, 10, None, "history").unwrap())
        );

        let items = history.snapshot();
        assert_eq!(items[0].id, "danmu:9:z later id:300");
        assert_eq!(items[0].received_seq, 0);
        assert_eq!(items[1].id, "danmu:1:a earlier id:100");
        assert_eq!(items[1].received_seq, 1);
    }

    #[test]
    fn sanitize_ffmpeg_error_redacts_stream_key_keeps_tail_and_limits_length() {
        let stderr = [
            "line 1",
            "line 2 key-secret",
            "line 3",
            "line 4 key-secret",
            "line 5",
        ]
        .join("\n");

        let sanitized = sanitize_ffmpeg_error(&stderr, "key-secret");

        assert!(!sanitized.contains("line 1"));
        assert!(sanitized.contains("line 2 <stream-key>"));
        assert!(sanitized.contains("line 4 <stream-key>"));
        assert!(!sanitized.contains("key-secret"));
        assert!(sanitized.len() <= 1200);
    }

    #[test]
    fn sanitize_ffmpeg_error_handles_empty_stderr() {
        assert_eq!(
            sanitize_ffmpeg_error(" \n\t ", "key"),
            "ffmpeg 没有返回错误详情"
        );
    }

    #[test]
    fn rtmp_close_warning_requires_broken_pipe_and_late_elapsed_time() {
        assert!(!is_rtmp_close_warning(
            "Error writing trailer: Connection reset",
            Duration::from_secs(TEST_STREAM_DURATION_SECONDS)
        ));
        assert!(!is_rtmp_close_warning(
            "Broken pipe",
            Duration::from_secs(TEST_STREAM_DURATION_SECONDS)
        ));
    }

    #[test]
    fn bili_errors_map_to_http_statuses() {
        let not_logged_in: ApiError = BiliError::NotLoggedIn.into();
        assert_eq!(not_logged_in.status, StatusCode::UNAUTHORIZED);

        let missing_config: ApiError = BiliError::MissingConfig("bili_jct").into();
        assert_eq!(missing_config.status, StatusCode::BAD_REQUEST);

        let api_error: ApiError = BiliError::Api {
            code: -1,
            message: "bad".to_string(),
        }
        .into();
        assert_eq!(api_error.status, StatusCode::BAD_GATEWAY);

        let invalid_response: ApiError = BiliError::InvalidResponse("data").into();
        assert_eq!(invalid_response.status, StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn api_error_response_serializes_json_body() {
        let response = ApiError::bad_request("bad request").into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();

        assert_eq!(
            serde_json::from_slice::<Value>(&body).unwrap(),
            json!({ "error": "bad request" })
        );
    }
}

impl ApiError {
    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }
}

impl From<BiliError> for ApiError {
    fn from(error: BiliError) -> Self {
        let status = match error {
            BiliError::NotLoggedIn => StatusCode::UNAUTHORIZED,
            BiliError::MissingConfig(_) => StatusCode::BAD_REQUEST,
            BiliError::Api { .. } => StatusCode::BAD_GATEWAY,
            BiliError::Http(_) | BiliError::Io(_) | BiliError::InvalidResponse(_) => {
                StatusCode::INTERNAL_SERVER_ERROR
            }
        };
        Self {
            status,
            message: error.to_string(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(json!({
                "error": self.message,
            })),
        )
            .into_response()
    }
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};

        let mut terminate = signal(SignalKind::terminate()).expect("install SIGTERM handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {},
            _ = terminate.recv() => {},
        }
    }

    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}
