// Copyright (C) 2026 Jamie Cui
// Author: Jamie Cui
// SPDX-License-Identifier: GPL-3.0-or-later

use super::{app_sign, wbi_sign};
use crate::config::{AppConfig, AppCookie, ConfigStore, StreamCredential, parse_cookie_header};
use reqwest::{
    Method,
    header::{CONTENT_TYPE, COOKIE, HeaderMap, ORIGIN, REFERER, SET_COOKIE, USER_AGENT},
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    time::{SystemTime, UNIX_EPOCH},
};
use thiserror::Error;
use url::form_urlencoded::Serializer;

const API_BASE: &str = "https://api.bilibili.com";
const LIVE_BASE: &str = "https://api.live.bilibili.com";
const PASSPORT_BASE: &str = "https://passport.bilibili.com";
const LIVE_ORIGIN: &str = "https://live.bilibili.com";
const USER_AGENT_VALUE: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/143.0.0.0 Safari/537.36 Edg/143.0.0.0";

pub type BiliResult<T> = Result<T, BiliError>;

#[derive(Debug, Error)]
pub enum BiliError {
    #[error("http request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("config save failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("bilibili api error: {message} ({code})")]
    Api { code: i64, message: String },
    #[error("not logged in")]
    NotLoggedIn,
    #[error("missing config: {0}")]
    MissingConfig(&'static str),
    #[error("invalid response: {0}")]
    InvalidResponse(&'static str),
}

#[derive(Debug, Clone)]
pub struct BiliRequest {
    pub base_url: &'static str,
    pub endpoint: String,
    pub method: Method,
    pub origin: Option<&'static str>,
    pub referer: Option<&'static str>,
    pub form: Vec<(&'static str, String)>,
    pub app_sign: bool,
    pub raw: bool,
}

#[derive(Debug, Serialize)]
pub struct LoginStatus {
    pub authenticated: bool,
    pub config: AppConfig,
    pub config_path: String,
}

#[derive(Debug, Serialize)]
pub struct BootstrapResponse {
    pub config: AppConfig,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct QrCode {
    pub url: String,
    pub qrcode_key: String,
}

#[derive(Clone)]
pub struct BiliClient {
    http: reqwest::Client,
    config: ConfigStore,
}

#[derive(Debug, Deserialize)]
struct BiliEnvelope<T> {
    code: i64,
    #[serde(default)]
    message: String,
    #[serde(default)]
    msg: String,
    data: Option<T>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UserInfo {
    mid: u64,
    uname: String,
    face: String,
    wbi_img: WbiImage,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WbiImage {
    img_url: String,
    sub_url: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RoomId {
    room_id: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DanmuInfo {
    token: String,
}

impl BiliClient {
    pub fn new(config: ConfigStore) -> BiliResult<Self> {
        let http = reqwest::Client::builder()
            .user_agent(USER_AGENT_VALUE)
            .redirect(reqwest::redirect::Policy::limited(10))
            .no_proxy()
            .build()?;
        Ok(Self { http, config })
    }

    pub async fn login_status(&self) -> LoginStatus {
        let config = self.config.get().await;
        LoginStatus {
            authenticated: config.cookie("SESSDATA").is_some()
                && config.cookie("bili_jct").is_some(),
            config,
            config_path: self.config.path().display().to_string(),
        }
    }

    pub async fn set_cookie_login(&self, cookie_header: &str) -> BiliResult<BootstrapResponse> {
        let cookies = parse_cookie_header(cookie_header);
        if !cookies.iter().any(|cookie| cookie.name == "SESSDATA")
            || !cookies.iter().any(|cookie| cookie.name == "bili_jct")
        {
            return Err(BiliError::NotLoggedIn);
        }

        self.config
            .update(|config| {
                config.clear_auth();
                config.set_cookies(cookies);
            })
            .await?;
        self.bootstrap().await
    }

    pub async fn logout(&self) -> BiliResult<AppConfig> {
        Ok(self.config.update(AppConfig::clear_auth).await?)
    }

    pub async fn bootstrap(&self) -> BiliResult<BootstrapResponse> {
        let user = self.user_info().await?;
        let room = self.room_id_by_uid(user.mid).await?;
        let areas = self.area_list().await?;

        let img_key = extract_key(&user.wbi_img.img_url);
        let sub_key = extract_key(&user.wbi_img.sub_url);
        let danmu = self
            .danmu_info_with_keys(room.room_id, &img_key, &sub_key)
            .await?;

        let config = self
            .config
            .update(|config| {
                config.uid = user.mid;
                config.username = Some(user.uname);
                config.avatar = Some(user.face);
                config.img_url = img_key;
                config.sub_url = sub_key;
                config.room_id = room.room_id;
                config.area_list = areas;
                config.room_token = danmu.token;
            })
            .await?;

        Ok(BootstrapResponse { config })
    }

    pub async fn qrcode_generate(&self) -> BiliResult<QrCode> {
        self.get_data(
            PASSPORT_BASE,
            "/x/passport-login/web/qrcode/generate",
            Some(PASSPORT_BASE),
            None,
        )
        .await
    }

    pub async fn qrcode_poll(&self, key: &str) -> BiliResult<Value> {
        let endpoint = format!("/x/passport-login/web/qrcode/poll?qrcode_key={key}");
        self.get_data(PASSPORT_BASE, &endpoint, Some(PASSPORT_BASE), None)
            .await
    }

    pub async fn user_info(&self) -> BiliResult<UserInfo> {
        self.get_data(API_BASE, "/x/web-interface/nav", Some(API_BASE), None)
            .await
    }

    pub async fn room_id_by_uid(&self, uid: u64) -> BiliResult<RoomId> {
        self.get_data(
            LIVE_BASE,
            &format!("/room/v2/Room/room_id_by_uid?uid={uid}"),
            Some(LIVE_BASE),
            None,
        )
        .await
    }

    pub async fn room_info(&self, room_id: u64) -> BiliResult<Value> {
        self.get_data(
            LIVE_BASE,
            &format!("/room/v1/Room/get_info?room_id={room_id}"),
            Some(LIVE_BASE),
            None,
        )
        .await
    }

    pub async fn area_list(&self) -> BiliResult<Value> {
        self.get_data(
            LIVE_BASE,
            "/room/v1/Area/getList?show_pinyin=1",
            Some(LIVE_BASE),
            None,
        )
        .await
    }

    pub async fn danmu_info(&self, room_id: u64) -> BiliResult<Value> {
        let config = self.config.get().await;
        let data = self
            .danmu_info_with_keys(room_id, &config.img_url, &config.sub_url)
            .await?;
        let value =
            serde_json::to_value(&data).map_err(|_| BiliError::InvalidResponse("danmu info"))?;
        self.config
            .update(|config| {
                config.room_id = room_id;
                config.room_token = data.token;
            })
            .await?;
        Ok(value)
    }

    async fn danmu_info_with_keys(
        &self,
        room_id: u64,
        img_key: &str,
        sub_key: &str,
    ) -> BiliResult<DanmuInfo> {
        if img_key.is_empty() || sub_key.is_empty() {
            return Err(BiliError::MissingConfig("wbi image keys"));
        }

        let query = wbi_sign(
            vec![("id", room_id.to_string()), ("type", "0".to_string())],
            img_key,
            sub_key,
        );
        self.get_data(
            LIVE_BASE,
            &format!("/xlive/web-room/v1/index/getDanmuInfo?{query}"),
            Some(LIVE_ORIGIN),
            Some("https://live.bilibili.com/"),
        )
        .await
    }

    pub async fn live_version(&self) -> BiliResult<Value> {
        let query = app_sign(vec![
            ("system_version", "2".to_string()),
            ("ts", unix_millis().to_string()),
        ]);
        self.get_data(
            LIVE_BASE,
            &format!("/xlive/app-blink/v1/liveVersionInfo/getHomePageLiveVersion?{query}"),
            Some(LIVE_BASE),
            None,
        )
        .await
    }

    pub async fn update_room_title(&self, title: String) -> BiliResult<Value> {
        let config = self.config.get().await;
        let csrf = csrf(&config)?;
        let value = self
            .post_data(
                LIVE_BASE,
                "/room/v1/Room/update",
                Some(LIVE_BASE),
                None,
                vec![
                    ("room_id", config.room_id.to_string()),
                    ("csrf", csrf.clone()),
                    ("csrf_token", csrf),
                    ("title", title.clone()),
                    ("platform", "pc_link".to_string()),
                ],
                false,
            )
            .await?;
        self.config
            .update(|config| config.room_title = title)
            .await?;
        Ok(value)
    }

    pub async fn update_room_area(&self, area_id: String) -> BiliResult<Value> {
        let config = self.config.get().await;
        let csrf = csrf(&config)?;
        let value = self
            .post_data(
                LIVE_BASE,
                "/room/v1/Room/update",
                Some(LIVE_BASE),
                None,
                vec![
                    ("room_id", config.room_id.to_string()),
                    ("csrf", csrf.clone()),
                    ("csrf_token", csrf),
                    ("area_id", area_id.clone()),
                    ("platform", "pc_link".to_string()),
                ],
                false,
            )
            .await?;
        self.config
            .update(|config| config.area_id = area_id)
            .await?;
        Ok(value)
    }

    pub async fn start_live(&self) -> BiliResult<Value> {
        let config = self.config.get().await;
        let csrf = csrf(&config)?;
        let response = self
            .send(BiliRequest {
                base_url: LIVE_BASE,
                endpoint: "/room/v1/Room/startLive".to_string(),
                method: Method::POST,
                origin: Some(LIVE_ORIGIN),
                referer: Some("https://live.bilibili.com/"),
                form: vec![
                    ("room_id", config.room_id.to_string()),
                    ("platform", "pc_link".to_string()),
                    ("csrf", csrf.clone()),
                    ("csrf_token", csrf),
                    ("area_v2", config.area_id),
                ],
                app_sign: false,
                raw: true,
            })
            .await?;

        if response.get("code").and_then(Value::as_i64) == Some(0) {
            let streams = parse_streams(response.get("data").unwrap_or(&Value::Null));
            self.config
                .update(|config| {
                    config.streams = streams;
                    config.is_open_live = true;
                })
                .await?;
        }

        Ok(response)
    }

    pub async fn stop_live(&self) -> BiliResult<Value> {
        let config = self.config.get().await;
        let csrf = csrf(&config)?;
        let value = self
            .post_data(
                LIVE_BASE,
                "/room/v1/Room/stopLive",
                Some(LIVE_BASE),
                None,
                vec![
                    ("room_id", config.room_id.to_string()),
                    ("csrf", csrf.clone()),
                    ("platform", "pc_link".to_string()),
                    ("csrf_token", csrf),
                ],
                false,
            )
            .await?;
        self.config
            .update(|config| {
                config.is_open_live = false;
                config.streams.clear();
            })
            .await?;
        Ok(value)
    }

    pub async fn send_comment(&self, message: String) -> BiliResult<Value> {
        let config = self.config.get().await;
        let csrf = csrf(&config)?;
        let query = wbi_sign(
            vec![("web_location", "444.8".to_string())],
            &config.img_url,
            &config.sub_url,
        );
        self.post_data(
            LIVE_BASE,
            &format!("/msg/send?{query}"),
            Some(LIVE_BASE),
            None,
            vec![
                ("msg", message),
                ("color", "16777215".to_string()),
                ("fontsize", "25".to_string()),
                ("rnd", unix_millis().to_string()),
                ("roomid", config.room_id.to_string()),
                ("csrf_token", csrf.clone()),
                ("csrf", csrf),
            ],
            false,
        )
        .await
    }

    pub async fn room_admins(&self, page: u64) -> BiliResult<Value> {
        self.get_data(
            LIVE_BASE,
            &format!("/xlive/app-ucenter/v1/roomAdmin/get_by_anchor?page={page}"),
            Some(LIVE_BASE),
            None,
        )
        .await
    }

    pub async fn add_room_admin(&self, uid: String) -> BiliResult<Value> {
        let csrf = csrf(&self.config.get().await)?;
        self.post_data(
            LIVE_BASE,
            "/xlive/web-ucenter/v1/roomAdmin/appoint",
            Some(LIVE_ORIGIN),
            None,
            vec![
                ("admin", uid),
                ("admin_level", "1".to_string()),
                ("csrf_token", csrf.clone()),
                ("csrf", csrf),
                ("visit_id", String::new()),
            ],
            false,
        )
        .await
    }

    pub async fn delete_room_admin(&self, uid: String) -> BiliResult<Value> {
        let csrf = csrf(&self.config.get().await)?;
        self.post_data(
            LIVE_BASE,
            "/xlive/app-ucenter/v1/roomAdmin/dismiss",
            Some(LIVE_BASE),
            None,
            vec![
                ("uid", uid),
                ("csrf_token", csrf.clone()),
                ("csrf", csrf),
                ("visit_id", String::new()),
            ],
            false,
        )
        .await
    }

    pub async fn silent_users(&self, page: u64) -> BiliResult<Value> {
        let config = self.config.get().await;
        let csrf = csrf(&config)?;
        self.post_data(
            LIVE_BASE,
            "/xlive/web-ucenter/v1/banned/GetSilentUserList",
            Some(LIVE_BASE),
            None,
            vec![
                ("room_id", config.room_id.to_string()),
                ("ps", page.to_string()),
                ("csrf_token", csrf.clone()),
                ("csrf", csrf),
                ("visit_id", String::new()),
            ],
            false,
        )
        .await
    }

    pub async fn search_users(&self, search: String) -> BiliResult<Value> {
        self.get_data(
            LIVE_BASE,
            &format!(
                "/banned_service/v2/Silent/search_user?search={}",
                encode_component(&search)
            ),
            Some(LIVE_BASE),
            None,
        )
        .await
    }

    pub async fn add_silent_user(&self, uid: String, hour: String) -> BiliResult<Value> {
        let config = self.config.get().await;
        let csrf = csrf(&config)?;
        self.post_data(
            LIVE_BASE,
            "/xlive/web-ucenter/v1/banned/AddSilentUser",
            Some(LIVE_BASE),
            None,
            vec![
                ("room_id", config.room_id.to_string()),
                ("tuid", uid),
                ("mobile_app", "web".to_string()),
                ("hour", hour),
                ("csrf_token", csrf.clone()),
                ("csrf", csrf),
                ("visit_id", String::new()),
            ],
            false,
        )
        .await
    }

    pub async fn delete_silent_user(&self, uid: String) -> BiliResult<Value> {
        let config = self.config.get().await;
        let csrf = csrf(&config)?;
        self.post_data(
            LIVE_BASE,
            "/xlive/web-ucenter/v1/banned/DelSilentUser",
            Some(LIVE_BASE),
            None,
            vec![
                ("room_id", config.room_id.to_string()),
                ("tuid", uid),
                ("mobi_app", "web".to_string()),
                ("csrf_token", csrf.clone()),
                ("csrf", csrf),
                ("visit_id", String::new()),
            ],
            false,
        )
        .await
    }

    pub async fn room_silent(&self) -> BiliResult<Value> {
        let room_id = self.config.get().await.room_id;
        self.get_data(
            LIVE_BASE,
            &format!("/xlive/web-room/v1/banned/GetRoomSilent?room_id={room_id}"),
            Some(LIVE_BASE),
            None,
        )
        .await
    }

    pub async fn set_room_silent(
        &self,
        kind: String,
        level: u64,
        minute: u64,
    ) -> BiliResult<Value> {
        let config = self.config.get().await;
        let csrf = csrf(&config)?;
        self.post_data(
            LIVE_BASE,
            "/xlive/web-room/v1/banned/RoomSilent",
            Some(LIVE_BASE),
            None,
            vec![
                ("room_id", config.room_id.to_string()),
                ("type", kind),
                ("level", level.to_string()),
                ("minute", minute.to_string()),
                ("csrf_token", csrf.clone()),
                ("csrf", csrf),
                ("visit_id", String::new()),
            ],
            false,
        )
        .await
    }

    pub async fn blocked_words(&self) -> BiliResult<Value> {
        let config = self.config.get().await;
        let csrf = csrf(&config)?;
        self.post_data(
            LIVE_BASE,
            "/xlive/web-ucenter/v1/banned/GetShieldKeywordList",
            Some(LIVE_BASE),
            None,
            vec![
                ("room_id", config.room_id.to_string()),
                ("csrf_token", csrf.clone()),
                ("csrf", csrf),
                ("visit_id", String::new()),
            ],
            false,
        )
        .await
    }

    pub async fn add_blocked_word(&self, keyword: String) -> BiliResult<Value> {
        let config = self.config.get().await;
        let csrf = csrf(&config)?;
        self.post_data(
            LIVE_BASE,
            "/xlive/web-ucenter/v1/banned/AddShieldKeyword",
            Some(LIVE_BASE),
            None,
            vec![
                ("room_id", config.room_id.to_string()),
                ("keyword", keyword),
                ("csrf_token", csrf.clone()),
                ("csrf", csrf),
                ("visit_id", String::new()),
            ],
            false,
        )
        .await
    }

    pub async fn delete_blocked_word(&self, keyword: String) -> BiliResult<Value> {
        let config = self.config.get().await;
        let csrf = csrf(&config)?;
        self.post_data(
            LIVE_BASE,
            "/xlive/web-ucenter/v1/banned/DelShieldKeyword",
            Some(LIVE_BASE),
            None,
            vec![
                ("room_id", config.room_id.to_string()),
                ("keyword", keyword),
                ("csrf_token", csrf.clone()),
                ("csrf", csrf),
                ("visit_id", String::new()),
            ],
            false,
        )
        .await
    }

    pub async fn contribution_rank(&self) -> BiliResult<Value> {
        let config = self.config.get().await;
        let query = wbi_sign(
            vec![
                ("ruid", config.uid.to_string()),
                ("room_id", config.room_id.to_string()),
                ("page", "1".to_string()),
                ("page_size", "100".to_string()),
                ("type", "online_rank".to_string()),
                ("switch", "contribution_rank".to_string()),
                ("platform", "web".to_string()),
                ("web_location", "444.8".to_string()),
            ],
            &config.img_url,
            &config.sub_url,
        );
        self.get_data(
            LIVE_BASE,
            &format!("/xlive/general-interface/v1/rank/queryContributionRank?{query}"),
            Some(LIVE_ORIGIN),
            Some("https://live.bilibili.com/"),
        )
        .await
    }

    pub async fn config(&self) -> AppConfig {
        self.config.get().await
    }

    pub async fn patch_config(&self, patch: Value) -> BiliResult<AppConfig> {
        Ok(self
            .config
            .update(|config| {
                if let Some(value) = patch.get("room_title").and_then(Value::as_str) {
                    config.room_title = value.to_string();
                }
                if let Some(value) = patch.get("category_id").and_then(Value::as_str) {
                    config.category_id = value.to_string();
                }
                if let Some(value) = patch.get("area_id").and_then(Value::as_str) {
                    config.area_id = value.to_string();
                }
                if let Some(value) = patch.get("theme").and_then(Value::as_str) {
                    config.theme = value.to_string();
                }
            })
            .await?)
    }

    async fn get_data<T: DeserializeOwned>(
        &self,
        base_url: &'static str,
        endpoint: &str,
        origin: Option<&'static str>,
        referer: Option<&'static str>,
    ) -> BiliResult<T> {
        let value = self
            .send(BiliRequest {
                base_url,
                endpoint: endpoint.to_string(),
                method: Method::GET,
                origin,
                referer,
                form: Vec::new(),
                app_sign: false,
                raw: false,
            })
            .await?;
        serde_json::from_value(value).map_err(|_| BiliError::InvalidResponse("data shape"))
    }

    async fn post_data(
        &self,
        base_url: &'static str,
        endpoint: &str,
        origin: Option<&'static str>,
        referer: Option<&'static str>,
        form: Vec<(&'static str, String)>,
        app_sign_body: bool,
    ) -> BiliResult<Value> {
        self.send(BiliRequest {
            base_url,
            endpoint: endpoint.to_string(),
            method: Method::POST,
            origin,
            referer,
            form,
            app_sign: app_sign_body,
            raw: false,
        })
        .await
    }

    pub async fn send(&self, request: BiliRequest) -> BiliResult<Value> {
        let url = format!("{}{}", request.base_url, request.endpoint);
        let config = self.config.get().await;

        let mut builder = self.http.request(request.method.clone(), url);
        builder = apply_common_headers(builder, &config, request.origin, request.referer);

        if request.method == Method::POST {
            let body = if request.app_sign {
                app_sign(request.form)
            } else {
                encode_form(
                    request
                        .form
                        .iter()
                        .map(|(key, value)| (*key, value.as_str())),
                )
            };
            builder = builder.header(
                CONTENT_TYPE,
                "application/x-www-form-urlencoded; charset=UTF-8",
            );
            builder = builder.body(body);
        }

        let response = builder.send().await?.error_for_status()?;
        let headers = response.headers().clone();
        let envelope = response.json::<BiliEnvelope<Value>>().await?;
        self.capture_cookies(&headers).await?;

        let message = if envelope.message.is_empty() {
            envelope.msg
        } else {
            envelope.message
        };

        if request.raw {
            return Ok(json!({
                "code": envelope.code,
                "message": message,
                "data": envelope.data.unwrap_or(Value::Null),
            }));
        }

        if envelope.code != 0 {
            return Err(BiliError::Api {
                code: envelope.code,
                message,
            });
        }

        envelope
            .data
            .ok_or(BiliError::InvalidResponse("missing data"))
    }

    async fn capture_cookies(&self, headers: &HeaderMap) -> BiliResult<()> {
        let mut cookies = Vec::new();
        for value in headers.get_all(SET_COOKIE) {
            let Ok(value) = value.to_str() else {
                continue;
            };
            if let Some(cookie) = parse_set_cookie(value) {
                cookies.push(cookie);
            }
        }

        if cookies.is_empty() {
            return Ok(());
        }

        self.config
            .update(|config| {
                config.set_cookies(cookies);
            })
            .await?;
        Ok(())
    }
}

fn apply_common_headers(
    mut builder: reqwest::RequestBuilder,
    config: &AppConfig,
    origin: Option<&'static str>,
    referer: Option<&'static str>,
) -> reqwest::RequestBuilder {
    builder = builder
        .header(USER_AGENT, USER_AGENT_VALUE)
        .header("accept", "*/*")
        .header(
            "sec-ch-ua",
            "\"Microsoft Edge\";v=\"143\", \"Chromium\";v=\"143\", \"Not A(Brand\";v=\"24\"",
        )
        .header("sec-ch-ua-mobile", "?0")
        .header("sec-ch-ua-platform", "\"Windows\"")
        .header("sec-fetch-dest", "empty")
        .header("sec-fetch-mode", "cors")
        .header("sec-fetch-site", "same-site");

    if let Some(origin) = origin {
        builder = builder.header(ORIGIN, origin);
    }
    if let Some(referer) = referer {
        builder = builder.header(REFERER, referer);
    }

    let cookie = config.cookie_header();
    if !cookie.is_empty() {
        builder = builder.header(COOKIE, cookie);
    }

    builder
}

fn csrf(config: &AppConfig) -> BiliResult<String> {
    config
        .cookie("bili_jct")
        .map(ToOwned::to_owned)
        .ok_or(BiliError::MissingConfig("bili_jct"))
}

fn parse_set_cookie(value: &str) -> Option<AppCookie> {
    let first = value.split(';').next()?;
    let (name, value) = first.split_once('=')?;
    let name = name.trim();
    let value = value.trim();
    if name.is_empty() || value.is_empty() {
        return None;
    }
    Some(AppCookie {
        name: name.to_string(),
        value: value.to_string(),
    })
}

fn parse_streams(data: &Value) -> Vec<StreamCredential> {
    let mut streams = Vec::new();
    let mut seen = HashSet::new();
    let mut rtmp = 0;
    let mut srt = 0;

    fn push_stream(
        streams: &mut Vec<StreamCredential>,
        seen: &mut HashSet<(String, String)>,
        rtmp: &mut usize,
        srt: &mut usize,
        protocol: &str,
        address: &str,
        key: &str,
    ) {
        let address = address.trim();
        let key = key.trim();
        if address.is_empty() || key.is_empty() {
            return;
        }

        if !seen.insert((address.to_string(), key.to_string())) {
            return;
        }

        let kind = match protocol {
            "rtmp" => {
                *rtmp += 1;
                format!("rtmp-{}", *rtmp)
            }
            "srt" => {
                *srt += 1;
                format!("srt-{}", *srt)
            }
            _ => return,
        };

        streams.push(StreamCredential {
            kind,
            address: address.to_string(),
            key: key.to_string(),
        });
    }

    if let Some(data_rtmp) = data.get("rtmp") {
        if let (Some(address), Some(key)) = (
            data_rtmp.get("addr").and_then(Value::as_str),
            data_rtmp.get("code").and_then(Value::as_str),
        ) {
            push_stream(
                &mut streams,
                &mut seen,
                &mut rtmp,
                &mut srt,
                "rtmp",
                address,
                key,
            );
        }
    }

    if let Some(protocols) = data.get("protocols").and_then(Value::as_array) {
        for protocol in protocols {
            let name = protocol
                .get("protocol")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let address = protocol
                .get("addr")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let key = protocol
                .get("code")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if address.is_empty() || key.is_empty() {
                continue;
            }

            push_stream(
                &mut streams,
                &mut seen,
                &mut rtmp,
                &mut srt,
                name,
                address,
                key,
            );
        }
    }

    streams
}

fn encode_form<'a>(pairs: impl Iterator<Item = (&'a str, &'a str)>) -> String {
    let mut serializer = Serializer::new(String::new());
    for (key, value) in pairs {
        serializer.append_pair(key, value);
    }
    serializer.finish()
}

fn encode_component(value: &str) -> String {
    encode_form(std::iter::once(("q", value)))
        .strip_prefix("q=")
        .unwrap_or_default()
        .to_string()
}

fn extract_key(value: &str) -> String {
    let filename = value.rsplit('/').next().unwrap_or(value);
    filename.split('.').next().unwrap_or(filename).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::{HeaderMap, HeaderValue};

    fn unique_config_path(name: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir()
            .join(format!(
                "bilive-client-{name}-{}-{nanos}",
                std::process::id()
            ))
            .join("config.json")
    }

    #[test]
    fn parses_unique_stream_credentials() {
        let data = json!({
            "rtmp": {
                "addr": "rtmp://live-push.bilivideo.com/live-bvc/",
                "code": "?streamname=live_1_2&key=secret&schedule=rtmp&pflag=2"
            },
            "protocols": [
                {
                    "protocol": "rtmp",
                    "addr": "rtmp://live-push.bilivideo.com/live-bvc/",
                    "code": "?streamname=live_1_2&key=secret&schedule=rtmp&pflag=2"
                }
            ]
        });

        let streams = parse_streams(&data);

        assert_eq!(
            streams,
            vec![StreamCredential {
                kind: "rtmp-1".to_string(),
                address: "rtmp://live-push.bilivideo.com/live-bvc/".to_string(),
                key: "?streamname=live_1_2&key=secret&schedule=rtmp&pflag=2".to_string(),
            }]
        );
    }

    #[test]
    fn keeps_distinct_stream_protocol_credentials() {
        let data = json!({
            "protocols": [
                {
                    "protocol": "rtmp",
                    "addr": "rtmp://live-push.bilivideo.com/live-bvc/",
                    "code": "?streamname=live_1_2&key=secret&schedule=rtmp&pflag=2"
                },
                {
                    "protocol": "srt",
                    "addr": "srt://live-push.bilivideo.com:1935",
                    "code": "?streamid=#!::h=live-push.bilivideo.com,r=live_1_2"
                }
            ]
        });

        let streams = parse_streams(&data);

        assert_eq!(streams.len(), 2);
        assert_eq!(streams[0].kind, "rtmp-1");
        assert_eq!(streams[1].kind, "srt-1");
    }

    #[test]
    fn parse_streams_trims_values_skips_invalid_and_numbers_by_protocol() {
        let data = json!({
            "protocols": [
                {
                    "protocol": "hls",
                    "addr": "https://example.test/live",
                    "code": "ignored"
                },
                {
                    "protocol": "rtmp",
                    "addr": " rtmp://example.test/live ",
                    "code": " key-1 "
                },
                {
                    "protocol": "srt",
                    "addr": "srt://example.test:1935",
                    "code": "?streamid=abc"
                },
                {
                    "protocol": "rtmp",
                    "addr": "rtmp://example.test/live2",
                    "code": "key-2"
                },
                {
                    "protocol": "rtmp",
                    "addr": "",
                    "code": "missing-address"
                }
            ]
        });

        let streams = parse_streams(&data);

        assert_eq!(
            streams,
            vec![
                StreamCredential {
                    kind: "rtmp-1".to_string(),
                    address: "rtmp://example.test/live".to_string(),
                    key: "key-1".to_string(),
                },
                StreamCredential {
                    kind: "srt-1".to_string(),
                    address: "srt://example.test:1935".to_string(),
                    key: "?streamid=abc".to_string(),
                },
                StreamCredential {
                    kind: "rtmp-2".to_string(),
                    address: "rtmp://example.test/live2".to_string(),
                    key: "key-2".to_string(),
                },
            ]
        );
    }

    #[test]
    fn parses_set_cookie_first_pair_only() {
        assert_eq!(
            parse_set_cookie("SESSDATA=abc; Path=/; HttpOnly"),
            Some(AppCookie {
                name: "SESSDATA".to_string(),
                value: "abc".to_string(),
            })
        );
        assert_eq!(parse_set_cookie("empty=; Path=/"), None);
        assert_eq!(parse_set_cookie("no-equals"), None);
    }

    #[test]
    fn csrf_reads_bili_jct_cookie() {
        let mut config = AppConfig::default();
        assert!(matches!(
            csrf(&config),
            Err(BiliError::MissingConfig("bili_jct"))
        ));

        config.set_cookies(vec![AppCookie {
            name: "bili_jct".to_string(),
            value: "csrf-token".to_string(),
        }]);
        assert_eq!(csrf(&config).unwrap(), "csrf-token");
    }

    #[test]
    fn encode_component_percent_encodes_query_values() {
        assert_eq!(encode_component("a b+c&汉"), "a+b%2Bc%26%E6%B1%89");
    }

    #[test]
    fn extract_key_handles_urls_and_plain_keys() {
        assert_eq!(
            extract_key("https://i0.hdslb.com/bfs/wbi/key-name.png"),
            "key-name"
        );
        assert_eq!(extract_key("plain-key"), "plain-key");
    }

    #[tokio::test]
    async fn login_status_requires_sessdata_and_csrf() {
        let path = unique_config_path("login-status");
        let parent = path.parent().unwrap().to_path_buf();
        let store = ConfigStore::load(Some(path.clone())).await.unwrap();
        let client = BiliClient::new(store.clone()).unwrap();

        assert!(!client.login_status().await.authenticated);

        store
            .update(|config| {
                config.set_cookies(vec![AppCookie {
                    name: "SESSDATA".to_string(),
                    value: "abc".to_string(),
                }]);
            })
            .await
            .unwrap();
        assert!(!client.login_status().await.authenticated);

        store
            .update(|config| {
                config.set_cookies(vec![AppCookie {
                    name: "bili_jct".to_string(),
                    value: "csrf".to_string(),
                }]);
            })
            .await
            .unwrap();
        let status = client.login_status().await;
        assert!(status.authenticated);
        assert_eq!(status.config_path, path.display().to_string());

        let _ = tokio::fs::remove_dir_all(parent).await;
    }

    #[tokio::test]
    async fn patch_config_updates_only_supported_string_fields() {
        let path = unique_config_path("patch");
        let parent = path.parent().unwrap().to_path_buf();
        let store = ConfigStore::load(Some(path.clone())).await.unwrap();
        store
            .update(|config| {
                config.uid = 42;
                config.room_id = 100;
            })
            .await
            .unwrap();
        let client = BiliClient::new(store.clone()).unwrap();

        let updated = client
            .patch_config(json!({
                "room_title": "New title",
                "category_id": "12",
                "area_id": "34",
                "theme": "dark",
                "uid": 999,
                "room_id": 888,
                "ignored": "value",
            }))
            .await
            .unwrap();

        assert_eq!(updated.room_title, "New title");
        assert_eq!(updated.category_id, "12");
        assert_eq!(updated.area_id, "34");
        assert_eq!(updated.theme, "dark");
        assert_eq!(updated.uid, 42);
        assert_eq!(updated.room_id, 100);

        let reloaded = ConfigStore::load(Some(path.clone())).await.unwrap();
        assert_eq!(reloaded.get().await.theme, "dark");

        let _ = tokio::fs::remove_dir_all(parent).await;
    }

    #[tokio::test]
    async fn set_cookie_login_rejects_missing_required_cookies_without_saving() {
        let path = unique_config_path("cookie-login");
        let parent = path.parent().unwrap().to_path_buf();
        let store = ConfigStore::load(Some(path)).await.unwrap();
        let client = BiliClient::new(store.clone()).unwrap();

        let error = client.set_cookie_login("SESSDATA=abc").await.unwrap_err();

        assert!(matches!(error, BiliError::NotLoggedIn));
        assert!(store.get().await.cookies.is_empty());

        let _ = tokio::fs::remove_dir_all(parent).await;
    }

    #[tokio::test]
    async fn capture_cookies_updates_store_and_csrf() {
        let path = unique_config_path("capture");
        let parent = path.parent().unwrap().to_path_buf();
        let store = ConfigStore::load(Some(path)).await.unwrap();
        let client = BiliClient::new(store.clone()).unwrap();
        let mut headers = HeaderMap::new();
        headers.append(
            SET_COOKIE,
            HeaderValue::from_static("SESSDATA=abc; Path=/; HttpOnly"),
        );
        headers.append(
            SET_COOKIE,
            HeaderValue::from_static("bili_jct=csrf; Path=/"),
        );
        headers.append(SET_COOKIE, HeaderValue::from_static("empty=; Path=/"));

        client.capture_cookies(&headers).await.unwrap();

        let config = store.get().await;
        assert_eq!(config.cookie("SESSDATA"), Some("abc"));
        assert_eq!(config.cookie("bili_jct"), Some("csrf"));
        assert_eq!(config.csrf.as_deref(), Some("csrf"));

        let _ = tokio::fs::remove_dir_all(parent).await;
    }
}

fn unix_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}
