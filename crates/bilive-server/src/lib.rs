use anyhow::Context;
use axum::{
    Json, Router,
    extract::{
        Path, Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::StatusCode,
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
    collections::HashMap,
    net::SocketAddr,
    path::PathBuf,
    process::Stdio,
    sync::Arc,
    time::{Duration, Instant},
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
    pub web_dir: PathBuf,
    pub config_path: Option<PathBuf>,
}

#[derive(Clone)]
struct AppState {
    events: broadcast::Sender<Event>,
    danmu_task: Arc<Mutex<Option<JoinHandle<()>>>>,
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
    let store = ConfigStore::load(config.config_path.clone())
        .await
        .context("failed to load config")?;
    let bili = BiliClient::new(store.clone()).context("failed to create bilibili client")?;
    let (events, _) = broadcast::channel(1024);
    let state = AppState {
        events,
        danmu_task: Arc::new(Mutex::new(None)),
        bili,
    };

    let app = Router::new()
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
        .route("/api/danmu/status", get(danmu_status))
        .fallback_service(static_service(config.web_dir.clone()))
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let listener = TcpListener::bind(config.listen)
        .await
        .with_context(|| format!("failed to bind {}", config.listen))?;

    info!(
        "bilive listening on http://{} with web dir {}",
        config.listen,
        config.web_dir.display()
    );
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
