// Copyright (C) 2026 Jamie Cui
// Author: Jamie Cui
// SPDX-License-Identifier: GPL-3.0-or-later

use anyhow::{Context, bail};
use bilive_core::{ConnectionStatus, Event};
use clap::Parser;
use futures_util::StreamExt;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    cmp::Ordering,
    collections::HashSet,
    io::{self, Read, Write},
    mem,
    sync::mpsc as std_mpsc,
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{sync::mpsc, time};
use tokio_tungstenite::{connect_async, tungstenite::Message as WsMessage};

const DEFAULT_URL: &str = "http://127.0.0.1:22333";
const DEFAULT_MAX_MESSAGES: usize = 500;
const HISTORY_REFRESH_SECS: u64 = 0;
const RECONNECT_SECS: u64 = 2;
const ESCAPE_SEQUENCE_TIMEOUT_MILLIS: i32 = 25;

#[derive(Debug, Parser)]
#[command(
    name = "bilive-danmu",
    version,
    about = "Deprecated terminal danmu viewer for a running bilive service. Use the web UI comments tab instead."
)]
struct Cli {
    /// Base URL for the local bilive service.
    #[arg(long, env = "BILIVE_URL", default_value = DEFAULT_URL)]
    url: String,

    /// Override the room id used when refreshing recent danmu history.
    #[arg(long)]
    room_id: Option<u64>,

    /// Maximum chat messages kept in the terminal buffer.
    #[arg(long, default_value_t = DEFAULT_MAX_MESSAGES)]
    max_messages: usize,

    /// Periodic recent-history refresh interval in seconds. The event stream updates immediately; use 0 to disable history polling.
    #[arg(long, default_value_t = HISTORY_REFRESH_SECS)]
    refresh_interval: u64,

    /// Do not request /api/danmu/connect on startup.
    #[arg(long)]
    no_connect: bool,

    /// Do not load recent danmu history on startup.
    #[arg(long)]
    no_history: bool,

    /// Include non-chat live-room events in the scrollback.
    #[arg(long)]
    show_system: bool,
}

#[derive(Debug, Clone)]
struct ServiceUrl {
    base: String,
}

#[derive(Clone)]
struct ApiClient {
    http: reqwest::Client,
    service: ServiceUrl,
}

#[derive(Debug, Clone, Deserialize)]
struct PublicConfig {
    room_id: u64,
    room_title: String,
    username: Option<String>,
    room_token_available: bool,
}

#[derive(Debug, Deserialize)]
struct DanmuStatusResponse {
    connected: bool,
}

#[derive(Debug, Deserialize)]
struct DanmuMessagesResponse {
    items: Vec<DanmuHistoryEntry>,
    recent_loaded: usize,
    recent_error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DanmuHistoryEntry {
    id: String,
    payload: String,
    received_at: u64,
    received_seq: u64,
    sent_at: Option<u64>,
}

#[derive(Debug)]
enum UiEvent {
    Event(Event),
    WsStatus(Result<(), String>),
    History(Result<HistoryResult, String>),
    Comment {
        message: String,
        result: Result<(), String>,
    },
    RefreshDue,
    Resize,
}

#[derive(Debug)]
struct HistoryResult {
    items: Vec<DanmuHistoryEntry>,
    recent_loaded: usize,
    recent_error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Input {
    Enter,
    Escape,
    Backspace,
    Delete,
    ClearInput,
    CursorLeft,
    CursorRight,
    Home,
    End,
    Up,
    Down,
    PageUp,
    PageDown,
    Char(char),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LineKind {
    Chat,
    Mine,
    SuperChat,
    System,
    Error,
}

#[derive(Debug, Clone)]
struct DanmuLine {
    id: String,
    sort_at: u64,
    sequence: u64,
    time: String,
    name: String,
    content: String,
    medal: Option<String>,
    price: Option<String>,
    kind: LineKind,
}

#[derive(Debug)]
struct RenderLine {
    text: String,
    kind: LineKind,
}

#[derive(Debug)]
struct App {
    service: ServiceUrl,
    room_id: u64,
    room_title: String,
    username: Option<String>,
    connected: bool,
    ws_connected: bool,
    show_system: bool,
    max_messages: usize,
    messages: Vec<DanmuLine>,
    seen: HashSet<String>,
    next_sequence: u64,
    chat_count: usize,
    system_count: usize,
    scroll: usize,
    status: String,
    status_kind: LineKind,
    refreshing: bool,
    input: String,
    input_cursor: usize,
    sending_comment: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Cli::parse();
    eprintln!(
        "warning: bilive-danmu is deprecated; use the web UI comments tab at the bilive service URL instead."
    );
    if args.max_messages == 0 {
        bail!("--max-messages must be greater than 0");
    }

    let service = ServiceUrl::new(&args.url);
    let api = ApiClient::new(service.clone());
    let config = api.config().await.with_context(|| {
        format!(
            "failed to read {}; start bilive first or pass --url",
            service.api_url("/api/config")
        )
    })?;
    let room_id = args.room_id.unwrap_or(config.room_id);

    let mut app = App::new(&args, service.clone(), room_id, config);
    if !args.no_connect {
        match api.connect_danmu(room_id).await {
            Ok(ConnectOutcome::Started) => {
                app.set_status("已请求连接弹幕", LineKind::System);
            }
            Ok(ConnectOutcome::AlreadyConnected) => {
                app.connected = true;
                app.set_status("弹幕已经连接", LineKind::System);
            }
            Err(error) => {
                app.set_status(format!("连接弹幕失败: {error}"), LineKind::Error);
            }
        }
    }

    match api.danmu_status().await {
        Ok(connected) => app.connected = connected,
        Err(error) => app.set_status(format!("读取弹幕状态失败: {error}"), LineKind::Error),
    }

    let mut terminal = Terminal::enter().context("failed to enter terminal mode")?;
    let (ui_tx, mut ui_rx) = mpsc::channel(256);
    let (input_tx, mut input_rx) = mpsc::channel(64);
    spawn_ws_task(service.ws_url("/api/events"), ui_tx.clone());
    spawn_input_thread(input_tx);
    spawn_resize_task(ui_tx.clone());

    if !args.no_history {
        app.refreshing = true;
        spawn_history_task(api.clone(), room_id, ui_tx.clone());
    }
    if args.refresh_interval > 0 {
        spawn_refresh_timer(args.refresh_interval, ui_tx.clone());
    }

    draw(&mut terminal, &mut app)?;

    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => break,
            Some(input) = input_rx.recv() => {
                if handle_input(input, &mut app, &api, room_id, &ui_tx) {
                    break;
                }
                draw(&mut terminal, &mut app)?;
            }
            Some(event) = ui_rx.recv() => {
                let should_draw = match event {
                    UiEvent::RefreshDue => {
                        refresh_history(&mut app, &api, room_id, &ui_tx);
                        true
                    }
                    UiEvent::Resize => true,
                    event => handle_ui_event(event, &mut app),
                };
                if should_draw {
                    draw(&mut terminal, &mut app)?;
                }
            }
        }
    }

    Ok(())
}

impl ServiceUrl {
    fn new(value: &str) -> Self {
        let trimmed = value.trim().trim_end_matches('/');
        let base = if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
            trimmed.to_string()
        } else {
            format!("http://{trimmed}")
        };
        Self { base }
    }

    fn api_url(&self, path: &str) -> String {
        format!("{}{}", self.base, path)
    }

    fn ws_url(&self, path: &str) -> String {
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

impl ApiClient {
    fn new(service: ServiceUrl) -> Self {
        Self {
            http: reqwest::Client::new(),
            service,
        }
    }

    async fn config(&self) -> anyhow::Result<PublicConfig> {
        Ok(self
            .http
            .get(self.service.api_url("/api/config"))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }

    async fn danmu_status(&self) -> anyhow::Result<bool> {
        let response: DanmuStatusResponse = self
            .http
            .get(self.service.api_url("/api/danmu/status"))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        Ok(response.connected)
    }

    async fn connect_danmu(&self, room_id: u64) -> anyhow::Result<ConnectOutcome> {
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

    async fn send_comment(&self, message: String) -> anyhow::Result<()> {
        let response = self
            .http
            .post(self.service.api_url("/api/live/comment"))
            .json(&json!({ "message": message }))
            .send()
            .await?;

        if response.status().is_success() {
            return Ok(());
        }

        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        bail!("HTTP {status}: {}", body.trim())
    }

    async fn history(&self, room_id: u64) -> anyhow::Result<HistoryResult> {
        let url = self.service.api_url(&format!(
            "/api/danmu/messages?room_id={room_id}&include_recent=1"
        ));
        let response: DanmuMessagesResponse = self
            .http
            .get(url)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        Ok(HistoryResult {
            items: response.items,
            recent_loaded: response.recent_loaded,
            recent_error: response.recent_error,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConnectOutcome {
    Started,
    AlreadyConnected,
}

impl App {
    fn new(args: &Cli, service: ServiceUrl, room_id: u64, config: PublicConfig) -> Self {
        let status = if config.room_token_available {
            "正在连接事件流".to_string()
        } else {
            "没有可用弹幕 token，请先在 Web UI 登录或刷新弹幕信息".to_string()
        };

        Self {
            service,
            room_id,
            room_title: config.room_title,
            username: config.username,
            connected: false,
            ws_connected: false,
            show_system: args.show_system,
            max_messages: args.max_messages,
            messages: Vec::new(),
            seen: HashSet::new(),
            next_sequence: 0,
            chat_count: 0,
            system_count: 0,
            scroll: 0,
            status,
            status_kind: LineKind::System,
            refreshing: false,
            input: String::new(),
            input_cursor: 0,
            sending_comment: false,
        }
    }

    fn set_status(&mut self, message: impl Into<String>, kind: LineKind) {
        self.status = message.into();
        self.status_kind = kind;
    }

    fn add_line(&mut self, line: DanmuLine) {
        if !self.seen.insert(line.id.clone()) {
            return;
        }

        match line.kind {
            LineKind::Chat | LineKind::Mine | LineKind::SuperChat => {
                self.chat_count = self.chat_count.saturating_add(1);
            }
            LineKind::System | LineKind::Error => {
                self.system_count = self.system_count.saturating_add(1);
            }
        }

        self.messages.push(line);
        self.messages.sort_by(compare_lines);
        self.trim_messages();
    }

    fn add_history(&mut self, result: HistoryResult) {
        let count_before = self.messages.len();
        for entry in result.items {
            if let Some(mut line) = chat_line_from_payload(&entry.payload, entry.received_at) {
                line.id = if entry.id.is_empty() {
                    line.id
                } else {
                    entry.id
                };
                line.sequence = entry.received_seq;
                line.sort_at = entry.sent_at.unwrap_or(line.sort_at);
                line.time = format_time(line.sort_at);
                self.add_line(line);
            }
        }

        let added = self.messages.len().saturating_sub(count_before);
        let mut status = format!(
            "已刷新历史，新增 {added} 条，接口加载 {}",
            result.recent_loaded
        );
        if let Some(error) = result.recent_error.filter(|value| !value.is_empty()) {
            status.push_str(&format!("；最近记录错误: {error}"));
            self.set_status(status, LineKind::Error);
        } else {
            self.set_status(status, LineKind::System);
        }
    }

    fn next_sequence(&mut self) -> u64 {
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.saturating_add(1);
        sequence
    }

    fn trim_messages(&mut self) {
        if self.messages.len() <= self.max_messages {
            return;
        }

        let excess = self.messages.len() - self.max_messages;
        self.messages.drain(0..excess);
        self.seen = self.messages.iter().map(|line| line.id.clone()).collect();
    }

    fn input_is_empty(&self) -> bool {
        self.input.trim().is_empty()
    }

    fn insert_input_char(&mut self, ch: char) {
        if ch.is_control() {
            return;
        }
        self.input.insert(self.input_cursor, ch);
        self.input_cursor += ch.len_utf8();
    }

    fn backspace_input(&mut self) {
        if self.input_cursor == 0 {
            return;
        }
        if let Some((index, _)) = self.input[..self.input_cursor].char_indices().last() {
            self.input.drain(index..self.input_cursor);
            self.input_cursor = index;
        }
    }

    fn delete_input(&mut self) {
        if self.input_cursor >= self.input.len() {
            return;
        }
        let end = self.input[self.input_cursor..]
            .chars()
            .next()
            .map(|ch| self.input_cursor + ch.len_utf8())
            .unwrap_or(self.input.len());
        self.input.drain(self.input_cursor..end);
    }

    fn move_input_left(&mut self) {
        if self.input_cursor == 0 {
            return;
        }
        if let Some((index, _)) = self.input[..self.input_cursor].char_indices().last() {
            self.input_cursor = index;
        }
    }

    fn move_input_right(&mut self) {
        if self.input_cursor >= self.input.len() {
            return;
        }
        self.input_cursor += self.input[self.input_cursor..]
            .chars()
            .next()
            .map(char::len_utf8)
            .unwrap_or_default();
    }

    fn clear_input(&mut self) {
        self.input.clear();
        self.input_cursor = 0;
    }

    fn input_cursor_end(&mut self) {
        self.input_cursor = self.input.len();
    }
}

fn compare_lines(left: &DanmuLine, right: &DanmuLine) -> Ordering {
    left.sort_at
        .cmp(&right.sort_at)
        .then_with(|| left.sequence.cmp(&right.sequence))
        .then_with(|| left.id.cmp(&right.id))
}

fn handle_input(
    input: Input,
    app: &mut App,
    api: &ApiClient,
    room_id: u64,
    ui_tx: &mpsc::Sender<UiEvent>,
) -> bool {
    match input {
        Input::Enter => {
            let message = app.input.trim().to_string();
            if message.is_empty() {
                app.set_status("请输入弹幕后按 Enter 发送", LineKind::System);
            } else if app.sending_comment {
                app.set_status("上一条弹幕正在发送", LineKind::System);
            } else {
                app.clear_input();
                app.sending_comment = true;
                app.set_status("正在发送弹幕", LineKind::System);
                spawn_comment_task(api.clone(), message, ui_tx.clone());
            }
        }
        Input::Escape => app.clear_input(),
        Input::Backspace => app.backspace_input(),
        Input::Delete => app.delete_input(),
        Input::ClearInput => app.clear_input(),
        Input::CursorLeft => app.move_input_left(),
        Input::CursorRight => app.move_input_right(),
        Input::Home => {
            if app.input_is_empty() {
                app.scroll = usize::MAX;
            } else {
                app.input_cursor = 0;
            }
        }
        Input::End => {
            if app.input_is_empty() {
                app.scroll = 0;
            } else {
                app.input_cursor_end();
            }
        }
        Input::Up => app.scroll = app.scroll.saturating_add(1),
        Input::Down => app.scroll = app.scroll.saturating_sub(1),
        Input::PageUp => app.scroll = app.scroll.saturating_add(10),
        Input::PageDown => app.scroll = app.scroll.saturating_sub(10),
        Input::Char(ch) => match ch {
            'q' | 'Q' if app.input_is_empty() => return true,
            'r' | 'R' if app.input_is_empty() => refresh_history(app, api, room_id, ui_tx),
            'k' | 'K' if app.input_is_empty() => app.scroll = app.scroll.saturating_add(1),
            'j' | 'J' if app.input_is_empty() => app.scroll = app.scroll.saturating_sub(1),
            'u' | 'U' if app.input_is_empty() => app.scroll = app.scroll.saturating_add(10),
            'd' | 'D' if app.input_is_empty() => app.scroll = app.scroll.saturating_sub(10),
            'g' if app.input_is_empty() => app.scroll = usize::MAX,
            'G' if app.input_is_empty() => app.scroll = 0,
            _ => app.insert_input_char(ch),
        },
    }

    false
}

fn refresh_history(app: &mut App, api: &ApiClient, room_id: u64, ui_tx: &mpsc::Sender<UiEvent>) {
    if app.refreshing {
        app.set_status("正在刷新历史", LineKind::System);
        return;
    }

    app.refreshing = true;
    app.set_status("正在刷新历史", LineKind::System);
    spawn_history_task(api.clone(), room_id, ui_tx.clone());
}

fn handle_ui_event(event: UiEvent, app: &mut App) -> bool {
    match event {
        UiEvent::Event(event) => match event {
            Event::Connection(status) => {
                app.connected = matches!(status, ConnectionStatus::Connected);
                app.set_status(connection_status_text(status), LineKind::System);
                true
            }
            Event::Error { message } => {
                app.set_status(format!("服务错误: {message}"), LineKind::Error);
                let now = now_millis();
                let sequence = app.next_sequence();
                app.add_line(DanmuLine {
                    id: format!("error:{now}:{message}"),
                    sort_at: now,
                    sequence,
                    time: format_time(now),
                    name: "服务错误".to_string(),
                    content: sanitize_text(&message),
                    medal: None,
                    price: None,
                    kind: LineKind::Error,
                });
                true
            }
            Event::DanmuRaw { payload } => {
                let now = now_millis();
                if let Some(mut line) = chat_line_from_payload(&payload, now) {
                    line.sequence = app.next_sequence();
                    app.add_line(line);
                    true
                } else if app.show_system {
                    let line = system_line_from_payload(&payload, now, app.next_sequence());
                    app.add_line(line);
                    true
                } else {
                    false
                }
            }
        },
        UiEvent::WsStatus(result) => match result {
            Ok(()) => {
                app.ws_connected = true;
                app.set_status("事件流已连接", LineKind::System);
                true
            }
            Err(message) => {
                app.ws_connected = false;
                app.set_status(format!("事件流断开: {message}"), LineKind::Error);
                true
            }
        },
        UiEvent::History(result) => {
            app.refreshing = false;
            match result {
                Ok(result) => app.add_history(result),
                Err(message) => app.set_status(format!("刷新历史失败: {message}"), LineKind::Error),
            }
            true
        }
        UiEvent::Comment { message, result } => {
            app.sending_comment = false;
            match result {
                Ok(()) => app.set_status("弹幕已发送，等待事件流回显", LineKind::System),
                Err(error) => {
                    app.input = message;
                    app.input_cursor_end();
                    app.set_status(format!("发送弹幕失败: {error}"), LineKind::Error);
                }
            }
            true
        }
        UiEvent::RefreshDue => false,
        UiEvent::Resize => true,
    }
}

fn spawn_ws_task(ws_url: String, ui_tx: mpsc::Sender<UiEvent>) {
    tokio::spawn(async move {
        loop {
            match connect_async(&ws_url).await {
                Ok((mut socket, _)) => {
                    let _ = ui_tx.send(UiEvent::WsStatus(Ok(()))).await;
                    while let Some(message) = socket.next().await {
                        match message {
                            Ok(WsMessage::Text(text)) => {
                                match serde_json::from_str::<Event>(text.as_ref()) {
                                    Ok(event) => {
                                        if ui_tx.send(UiEvent::Event(event)).await.is_err() {
                                            return;
                                        }
                                    }
                                    Err(error) => {
                                        let _ = ui_tx
                                            .send(UiEvent::WsStatus(Err(format!(
                                                "事件解析失败: {error}"
                                            ))))
                                            .await;
                                    }
                                }
                            }
                            Ok(WsMessage::Close(_)) => break,
                            Ok(_) => {}
                            Err(error) => {
                                let _ = ui_tx.send(UiEvent::WsStatus(Err(error.to_string()))).await;
                                break;
                            }
                        }
                    }
                }
                Err(error) => {
                    let _ = ui_tx.send(UiEvent::WsStatus(Err(error.to_string()))).await;
                }
            }

            time::sleep(Duration::from_secs(RECONNECT_SECS)).await;
        }
    });
}

fn spawn_history_task(api: ApiClient, room_id: u64, ui_tx: mpsc::Sender<UiEvent>) {
    tokio::spawn(async move {
        let result = api
            .history(room_id)
            .await
            .map_err(|error| error.to_string());
        let _ = ui_tx.send(UiEvent::History(result)).await;
    });
}

fn spawn_comment_task(api: ApiClient, message: String, ui_tx: mpsc::Sender<UiEvent>) {
    tokio::spawn(async move {
        let result = api
            .send_comment(message.clone())
            .await
            .map_err(|error| error.to_string());
        let _ = ui_tx.send(UiEvent::Comment { message, result }).await;
    });
}

fn spawn_refresh_timer(refresh_interval: u64, ui_tx: mpsc::Sender<UiEvent>) {
    tokio::spawn(async move {
        let mut tick = time::interval(Duration::from_secs(refresh_interval));
        tick.tick().await;
        loop {
            tick.tick().await;
            if ui_tx.send(UiEvent::RefreshDue).await.is_err() {
                break;
            }
        }
    });
}

fn spawn_resize_task(ui_tx: mpsc::Sender<UiEvent>) {
    #[cfg(unix)]
    tokio::spawn(async move {
        let Ok(mut signal) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::window_change())
        else {
            return;
        };
        while signal.recv().await.is_some() {
            if ui_tx.send(UiEvent::Resize).await.is_err() {
                break;
            }
        }
    });

    #[cfg(not(unix))]
    {
        let _ = ui_tx;
    }
}

fn spawn_input_thread(input_tx: mpsc::Sender<Input>) {
    let (thread_tx, thread_rx) = std_mpsc::channel();
    thread::spawn(move || {
        read_input(thread_tx);
    });

    thread::spawn(move || {
        while let Ok(input) = thread_rx.recv() {
            if input_tx.blocking_send(input).is_err() {
                break;
            }
        }
    });
}

fn read_input(input_tx: std_mpsc::Sender<Input>) {
    let mut stdin = io::stdin();
    loop {
        let mut byte = [0u8; 1];
        if stdin.read_exact(&mut byte).is_err() {
            break;
        }

        let input = match byte[0] {
            b'\r' | b'\n' => Some(Input::Enter),
            0x7f | 0x08 => Some(Input::Backspace),
            0x04 => Some(Input::Delete),
            0x15 => Some(Input::ClearInput),
            0x1b => read_escape_input(&mut stdin),
            value if value.is_ascii() => Some(Input::Char(value as char)),
            value => read_utf8_char(value, &mut stdin).map(Input::Char),
        };

        if let Some(input) = input {
            if input_tx.send(input).is_err() {
                break;
            }
        }
    }
}

fn read_escape_input(stdin: &mut io::Stdin) -> Option<Input> {
    let Some(first) = read_byte_timeout(stdin, ESCAPE_SEQUENCE_TIMEOUT_MILLIS) else {
        return Some(Input::Escape);
    };
    if first != b'[' && first != b'O' {
        return None;
    }

    let second = read_byte_timeout(stdin, ESCAPE_SEQUENCE_TIMEOUT_MILLIS)?;
    match (first, second) {
        (b'[', b'A') => Some(Input::Up),
        (b'[', b'B') => Some(Input::Down),
        (b'[', b'C') => Some(Input::CursorRight),
        (b'[', b'D') => Some(Input::CursorLeft),
        (b'[', b'H') | (b'O', b'H') => Some(Input::Home),
        (b'[', b'F') | (b'O', b'F') => Some(Input::End),
        (b'[', b'1') | (b'[', b'7') => read_tilde_key(stdin, Input::Home),
        (b'[', b'4') | (b'[', b'8') => read_tilde_key(stdin, Input::End),
        (b'[', b'3') => read_tilde_key(stdin, Input::Delete),
        (b'[', b'5') => read_tilde_key(stdin, Input::PageUp),
        (b'[', b'6') => read_tilde_key(stdin, Input::PageDown),
        _ => None,
    }
}

fn read_tilde_key(stdin: &mut io::Stdin, input: Input) -> Option<Input> {
    match read_byte_timeout(stdin, ESCAPE_SEQUENCE_TIMEOUT_MILLIS) {
        Some(b'~') | None => Some(input),
        Some(_) => None,
    }
}

fn read_byte_timeout(stdin: &mut io::Stdin, timeout_millis: i32) -> Option<u8> {
    let mut pollfd = libc::pollfd {
        fd: libc::STDIN_FILENO,
        events: libc::POLLIN,
        revents: 0,
    };
    let ready = unsafe { libc::poll(&mut pollfd, 1, timeout_millis) };
    if ready <= 0 || pollfd.revents & libc::POLLIN == 0 {
        return None;
    }

    let mut byte = [0u8; 1];
    stdin.read_exact(&mut byte).ok()?;
    Some(byte[0])
}

fn read_utf8_char(first: u8, stdin: &mut io::Stdin) -> Option<char> {
    let width = match first {
        0xc2..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf4 => 4,
        _ => return None,
    };
    let mut buffer = [0u8; 4];
    buffer[0] = first;
    for slot in buffer.iter_mut().take(width).skip(1) {
        *slot = read_byte_timeout(stdin, ESCAPE_SEQUENCE_TIMEOUT_MILLIS)?;
    }
    std::str::from_utf8(&buffer[..width]).ok()?.chars().next()
}

fn chat_line_from_payload(payload: &str, received_at: u64) -> Option<DanmuLine> {
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
    let medal = info.get(3).and_then(Value::as_array);
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

    let medal = medal.and_then(|medal| {
        let name = medal.get(1).map(value_to_plain_string).unwrap_or_default();
        if name.is_empty() {
            return None;
        }
        let level = medal.first().map(value_to_plain_string).unwrap_or_default();
        Some(format!("{name} {level}").trim().to_string())
    });
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
        time: format_time(sort_at),
        name: sanitize_text(&name),
        content: sanitize_text(&content),
        medal: medal.map(|value| sanitize_text(&value)),
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
        time: format_time(sort_at),
        name: sanitize_text(&name),
        content: sanitize_text(&content),
        medal: Some("醒目留言".to_string()),
        price,
        kind: LineKind::SuperChat,
    })
}

fn system_line_from_payload(payload: &str, received_at: u64, sequence: u64) -> DanmuLine {
    let event = describe_system_event(payload);
    DanmuLine {
        id: format!("system:{received_at}:{sequence}:{}", event.title),
        sort_at: received_at,
        sequence,
        time: format_time(received_at),
        name: event.title,
        content: event.body,
        medal: None,
        price: None,
        kind: LineKind::System,
    }
}

#[derive(Debug)]
struct SystemEventText {
    title: String,
    body: String,
}

fn describe_system_event(raw: &str) -> SystemEventText {
    let Ok(parsed) = serde_json::from_str::<Value>(raw) else {
        return SystemEventText {
            title: "原始事件".to_string(),
            body: sanitize_text(&raw.chars().take(240).collect::<String>()),
        };
    };

    let data = parsed.get("data").unwrap_or(&Value::Null);
    let cmd = parsed
        .get("cmd")
        .and_then(Value::as_str)
        .unwrap_or_default();
    match cmd {
        "ONLINE_RANK_COUNT" => SystemEventText {
            title: "在线人数".to_string(),
            body: field_text(data, &["online_count_text", "count_text"])
                .or_else(|| field_text(data, &["online_count", "count"]))
                .unwrap_or_else(|| "已更新".to_string()),
        },
        "WATCHED_CHANGE" => SystemEventText {
            title: "看过人数".to_string(),
            body: field_text(data, &["text_large", "text_small"])
                .unwrap_or_else(|| "已更新".to_string()),
        },
        "INTERACT_WORD" | "INTERACT_WORD_V2" => {
            let name = field_text(data, &["uname"]).unwrap_or_else(|| "用户".to_string());
            SystemEventText {
                title: "互动".to_string(),
                body: format!("{name} 进入直播间"),
            }
        }
        "SEND_GIFT" => {
            let name = field_text(data, &["uname"]).unwrap_or_else(|| "用户".to_string());
            let gift = field_text(data, &["giftName"]).unwrap_or_else(|| "礼物".to_string());
            let num = field_text(data, &["num"]).unwrap_or_else(|| "1".to_string());
            SystemEventText {
                title: "礼物".to_string(),
                body: format!("{name} 送出 {gift} x{num}"),
            }
        }
        "ONLINE_RANK_V3" => SystemEventText {
            title: "在线榜单".to_string(),
            body: "榜单已更新".to_string(),
        },
        "STOP_LIVE_ROOM_LIST" => SystemEventText {
            title: "推荐列表".to_string(),
            body: "列表已更新".to_string(),
        },
        _ => SystemEventText {
            title: if cmd.is_empty() {
                "系统事件".to_string()
            } else {
                format!("事件 {cmd}")
            },
            body: sanitize_text(&raw.chars().take(240).collect::<String>()),
        },
    }
}

fn field_text(value: &Value, names: &[&str]) -> Option<String> {
    names
        .iter()
        .find_map(|name| value.get(*name))
        .map(value_to_plain_string)
        .filter(|value| !value.is_empty())
        .map(|value| sanitize_text(&value))
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

fn connection_status_text(status: ConnectionStatus) -> &'static str {
    match status {
        ConnectionStatus::Connecting => "弹幕连接中",
        ConnectionStatus::Connected => "弹幕已连接",
        ConnectionStatus::Disconnected => "弹幕已断开",
    }
}

fn draw(terminal: &mut Terminal, app: &mut App) -> io::Result<()> {
    let (cols, rows) = terminal.size();
    let width = cols.max(40) as usize;
    let height = rows.max(8) as usize;
    let body_height = height.saturating_sub(5);
    let rendered = render_lines(app, width.saturating_sub(1));
    let max_scroll = rendered.len().saturating_sub(body_height);
    app.scroll = app.scroll.min(max_scroll);
    let start = rendered.len().saturating_sub(body_height + app.scroll);
    let end = (start + body_height).min(rendered.len());
    let (input, input_cursor) = input_text(app, width);

    terminal.write("\x1b[?25l\x1b[H")?;
    terminal.clear_line()?;
    terminal.write_styled(&header_text(app, width), LineKind::System)?;

    terminal.write("\r\n")?;
    terminal.clear_line()?;
    terminal.write_styled(
        &truncate_to_width(&app.status, width.saturating_sub(1)),
        app.status_kind,
    )?;

    terminal.write("\r\n")?;
    terminal.clear_line()?;
    terminal.write(&"-".repeat(width.saturating_sub(1)))?;

    if rendered.is_empty() {
        let empty_at = body_height / 2;
        for row in 0..body_height {
            terminal.write("\r\n")?;
            terminal.clear_line()?;
            if row == empty_at {
                terminal.write_styled("暂无弹幕", LineKind::System)?;
            }
        }
    } else {
        for line in &rendered[start..end] {
            terminal.write("\r\n")?;
            terminal.clear_line()?;
            terminal.write_styled(&line.text, line.kind)?;
        }
        for _ in (end - start)..body_height {
            terminal.write("\r\n")?;
            terminal.clear_line()?;
        }
    }

    terminal.write("\r\n")?;
    terminal.clear_line()?;
    terminal.write_styled(&footer_text(app, max_scroll, width), LineKind::System)?;
    terminal.write("\r\n")?;
    terminal.clear_line()?;
    terminal.write(&input)?;
    terminal.write(&format!(
        "\x1b[{height};{}H\x1b[?25h",
        input_cursor.saturating_add(1)
    ))?;
    terminal.flush()
}

fn header_text(app: &App, width: usize) -> String {
    let connected = if app.connected {
        "弹幕:已连"
    } else {
        "弹幕:未连"
    };
    let events = if app.ws_connected {
        "事件流:已连"
    } else {
        "事件流:断开"
    };
    let title = if app.room_title.is_empty() {
        "未命名直播间".to_string()
    } else {
        app.room_title.clone()
    };
    let user = app.username.as_deref().unwrap_or("未登录用户");
    truncate_to_width(
        &format!(
            "bilive-danmu | {} | room {} | {} | {} | {}",
            app.service.base, app.room_id, connected, events, user
        ),
        width.saturating_sub(display_width(&title) + 3),
    ) + " | "
        + &truncate_to_width(&title, width / 3)
}

fn footer_text(app: &App, max_scroll: usize, width: usize) -> String {
    let refreshing = if app.refreshing {
        " | 正在刷新"
    } else {
        ""
    };
    let sending = if app.sending_comment {
        " | 正在发送"
    } else {
        ""
    };
    truncate_to_width(
        &format!(
            "空输入时 q 退出 r 刷新 k/j 滚动 u/d 翻页 g/G 首尾 | 弹幕 {} 系统 {} 滚动 {}/{}{}{}",
            app.chat_count, app.system_count, app.scroll, max_scroll, refreshing, sending
        ),
        width.saturating_sub(1),
    )
}

fn input_text(app: &App, width: usize) -> (String, usize) {
    let prefix = "弹幕> ";
    let line_width = width.saturating_sub(1);
    let prefix_width = display_width(prefix).min(line_width);
    let content_width = line_width.saturating_sub(prefix_width);
    let start = input_start_for_cursor(&app.input, app.input_cursor, content_width);
    let visible = truncate_to_width(&app.input[start..], content_width);
    let cursor_offset = display_width(&app.input[start..app.input_cursor]).min(content_width);
    let cursor_col = prefix_width.saturating_add(cursor_offset).min(line_width);

    let mut line = truncate_to_width(prefix, prefix_width);
    line.push_str(&visible);
    (line, cursor_col)
}

fn input_start_for_cursor(input: &str, cursor: usize, width: usize) -> usize {
    if width == 0 || display_width(&input[..cursor]) <= width {
        return 0;
    }

    let mut used = 0usize;
    let mut start = cursor;
    for (index, ch) in input[..cursor].char_indices().rev() {
        let ch_width = char_width(ch);
        if used + ch_width > width {
            break;
        }
        used += ch_width;
        start = index;
    }
    start
}

fn render_lines(app: &App, width: usize) -> Vec<RenderLine> {
    let mut lines = Vec::new();
    for message in &app.messages {
        let mut prefix = format!("[{}] ", message.time);
        if matches!(message.kind, LineKind::SuperChat) {
            prefix.push_str("[SC] ");
        }
        if let Some(price) = &message.price {
            prefix.push_str(&format!("[{price}] "));
        }
        if let Some(medal) = &message.medal {
            prefix.push_str(&format!("[{medal}] "));
        }
        prefix.push_str(&format!("{}: ", message.name));

        let continuation = " ".repeat(display_width(&prefix).min(width.saturating_sub(1)));
        let wrapped = wrap_with_prefix(&prefix, &message.content, &continuation, width);
        for text in wrapped {
            lines.push(RenderLine {
                text,
                kind: message.kind,
            });
        }
    }
    lines
}

fn wrap_with_prefix(prefix: &str, content: &str, continuation: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![String::new()];
    }

    let mut lines = Vec::new();
    let mut line = String::new();
    let mut line_width = 0usize;
    push_visible(&mut line, &mut line_width, prefix, width);

    let mut first_char = true;
    for ch in content.chars() {
        if ch == '\n' {
            lines.push(truncate_to_width(&line, width));
            line.clear();
            line_width = 0;
            push_visible(&mut line, &mut line_width, continuation, width);
            first_char = false;
            continue;
        }

        let ch_width = char_width(ch);
        if line_width + ch_width > width && !first_char {
            lines.push(truncate_to_width(&line, width));
            line.clear();
            line_width = 0;
            push_visible(&mut line, &mut line_width, continuation, width);
        }
        line.push(ch);
        line_width += ch_width;
        first_char = false;
    }

    lines.push(truncate_to_width(&line, width));
    lines
}

fn push_visible(line: &mut String, line_width: &mut usize, value: &str, width: usize) {
    for ch in value.chars() {
        let ch_width = char_width(ch);
        if *line_width + ch_width > width {
            break;
        }
        line.push(ch);
        *line_width += ch_width;
    }
}

fn truncate_to_width(value: &str, width: usize) -> String {
    let mut result = String::new();
    let mut used = 0usize;
    for ch in value.chars() {
        let ch_width = char_width(ch);
        if used + ch_width > width {
            break;
        }
        result.push(ch);
        used += ch_width;
    }
    result
}

fn display_width(value: &str) -> usize {
    value.chars().map(char_width).sum()
}

fn char_width(ch: char) -> usize {
    if ch.is_control() {
        return 0;
    }
    match ch as u32 {
        0x1100..=0x115f
        | 0x2329..=0x232a
        | 0x2e80..=0xa4cf
        | 0xac00..=0xd7a3
        | 0xf900..=0xfaff
        | 0xfe10..=0xfe19
        | 0xfe30..=0xfe6f
        | 0xff00..=0xff60
        | 0xffe0..=0xffe6 => 2,
        _ => 1,
    }
}

fn sanitize_text(value: &str) -> String {
    value
        .chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect()
}

fn now_millis() -> u64 {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    u64::try_from(millis).unwrap_or(u64::MAX)
}

fn format_time(millis: u64) -> String {
    let seconds = (millis / 1000) as libc::time_t;
    let mut tm = mem::MaybeUninit::<libc::tm>::uninit();
    let formatted = unsafe {
        if libc::localtime_r(&seconds, tm.as_mut_ptr()).is_null() {
            None
        } else {
            let tm = tm.assume_init();
            Some(format!(
                "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
                tm.tm_year + 1900,
                tm.tm_mon + 1,
                tm.tm_mday,
                tm.tm_hour,
                tm.tm_min,
                tm.tm_sec
            ))
        }
    };
    formatted.unwrap_or_else(|| "0000-00-00 00:00:00".to_string())
}

struct Terminal {
    stdout: io::Stdout,
    original: libc::termios,
}

impl Terminal {
    fn enter() -> io::Result<Self> {
        let mut original = mem::MaybeUninit::<libc::termios>::uninit();
        let result = unsafe { libc::tcgetattr(libc::STDIN_FILENO, original.as_mut_ptr()) };
        if result != 0 {
            return Err(io::Error::last_os_error());
        }
        let original = unsafe { original.assume_init() };
        let mut raw = original;
        raw.c_lflag &= !(libc::ECHO | libc::ICANON);
        raw.c_iflag &= !(libc::IXON | libc::ICRNL);
        raw.c_cc[libc::VMIN] = 1;
        raw.c_cc[libc::VTIME] = 0;
        let result = unsafe { libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &raw) };
        if result != 0 {
            return Err(io::Error::last_os_error());
        }

        let mut terminal = Self {
            stdout: io::stdout(),
            original,
        };
        terminal.write("\x1b[?1049h\x1b[2J\x1b[H\x1b[?25l")?;
        terminal.flush()?;
        Ok(terminal)
    }

    fn size(&self) -> (u16, u16) {
        let mut size = mem::MaybeUninit::<libc::winsize>::uninit();
        let result =
            unsafe { libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, size.as_mut_ptr()) };
        if result != 0 {
            return (80, 24);
        }
        let size = unsafe { size.assume_init() };
        let cols = if size.ws_col == 0 { 80 } else { size.ws_col };
        let rows = if size.ws_row == 0 { 24 } else { size.ws_row };
        (cols, rows)
    }

    fn clear_line(&mut self) -> io::Result<()> {
        self.write("\x1b[2K")
    }

    fn write_styled(&mut self, value: &str, kind: LineKind) -> io::Result<()> {
        self.write(style_code(kind))?;
        self.write(value)?;
        self.write("\x1b[0m")
    }

    fn write(&mut self, value: &str) -> io::Result<()> {
        self.stdout.write_all(value.as_bytes())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.stdout.flush()
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        let _ = unsafe { libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &self.original) };
        let _ = self.write("\x1b[0m\x1b[?25h\x1b[?1049l");
        let _ = self.flush();
    }
}

fn style_code(kind: LineKind) -> &'static str {
    match kind {
        LineKind::Chat => "\x1b[37m",
        LineKind::Mine => "\x1b[36m",
        LineKind::SuperChat => "\x1b[33;1m",
        LineKind::System => "\x1b[90m",
        LineKind::Error => "\x1b[31;1m",
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
    fn parses_plain_danmu_message() {
        let line = chat_line_from_payload(
            r#"{"cmd":"DANMU_MSG","info":[[0,1,25,0,1780000000],"你好",[42,"Jamie"],["7","牌子"]]}"#,
            1,
        )
        .unwrap();

        assert_eq!(line.name, "Jamie");
        assert_eq!(line.content, "你好");
        assert_eq!(line.medal.as_deref(), Some("牌子 7"));
        assert_eq!(line.sort_at, 1_780_000_000_000);
    }

    #[test]
    fn history_time_uses_standard_local_format() {
        let mut app = App::new(
            &Cli {
                url: DEFAULT_URL.to_string(),
                room_id: None,
                max_messages: DEFAULT_MAX_MESSAGES,
                refresh_interval: 0,
                no_connect: true,
                no_history: true,
                show_system: false,
            },
            ServiceUrl::new(DEFAULT_URL),
            1,
            PublicConfig {
                room_id: 1,
                room_title: "room".to_string(),
                username: None,
                room_token_available: true,
            },
        );
        app.add_history(HistoryResult {
            items: vec![DanmuHistoryEntry {
                id: "danmu:42:你好:1780000000".to_string(),
                payload:
                    r#"{"cmd":"DANMU_MSG","info":[[0,1,25,0,1780000000],"你好",[42,"Jamie"],[]]}"#
                        .to_string(),
                received_at: 1,
                received_seq: 0,
                sent_at: Some(1_780_000_000_000),
            }],
            recent_loaded: 1,
            recent_error: None,
        });

        assert_eq!(app.messages[0].time.len(), 19);
        assert!(app.messages[0].time.chars().all(|ch| ch.is_ascii()));
        assert_ne!(app.messages[0].time, "2026-04-30 12:00:00");
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
    fn wraps_cjk_text_by_display_width() {
        let lines = wrap_with_prefix("[00] A: ", "你好世界", "        ", 12);

        assert_eq!(lines, vec!["[00] A: 你好", "        世界"]);
    }

    #[test]
    fn scrollback_order_keeps_latest_at_bottom() {
        let mut app = App::new(
            &Cli {
                url: DEFAULT_URL.to_string(),
                room_id: None,
                max_messages: DEFAULT_MAX_MESSAGES,
                refresh_interval: 0,
                no_connect: true,
                no_history: true,
                show_system: false,
            },
            ServiceUrl::new(DEFAULT_URL),
            1,
            PublicConfig {
                room_id: 1,
                room_title: "room".to_string(),
                username: None,
                room_token_available: true,
            },
        );
        app.add_line(DanmuLine {
            id: "late".to_string(),
            sort_at: 2,
            sequence: 0,
            time: format_time(2),
            name: "u".to_string(),
            content: "late".to_string(),
            medal: None,
            price: None,
            kind: LineKind::Chat,
        });
        app.add_line(DanmuLine {
            id: "early".to_string(),
            sort_at: 1,
            sequence: 1,
            time: format_time(1),
            name: "u".to_string(),
            content: "early".to_string(),
            medal: None,
            price: None,
            kind: LineKind::Chat,
        });

        let rendered = render_lines(&app, 120);
        assert!(rendered.first().unwrap().text.ends_with("early"));
        assert!(rendered.last().unwrap().text.ends_with("late"));
    }

    #[test]
    fn input_view_tracks_cursor_with_cjk_text() {
        let mut app = App::new(
            &Cli {
                url: DEFAULT_URL.to_string(),
                room_id: None,
                max_messages: DEFAULT_MAX_MESSAGES,
                refresh_interval: 0,
                no_connect: true,
                no_history: true,
                show_system: false,
            },
            ServiceUrl::new(DEFAULT_URL),
            1,
            PublicConfig {
                room_id: 1,
                room_title: "room".to_string(),
                username: None,
                room_token_available: true,
            },
        );
        app.input = "你好 bilive".to_string();
        app.input_cursor = "你好".len();

        let (line, cursor) = input_text(&app, 80);
        assert_eq!(line, "弹幕> 你好 bilive");
        assert_eq!(cursor, display_width("弹幕> 你好"));
    }
}
