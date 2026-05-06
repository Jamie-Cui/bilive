// Copyright (C) 2026 Jamie Cui
// Author: Jamie Cui
// SPDX-License-Identifier: GPL-3.0-or-later

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    env,
    ffi::OsString,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AppCookie {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StreamCredential {
    #[serde(rename = "type")]
    pub kind: String,
    pub address: String,
    pub key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct DanmuNotificationConfig {
    pub enabled: bool,
    pub danmu: bool,
    pub super_chat: bool,
    pub cooldown_secs: u64,
    pub expire_timeout_ms: u64,
}

impl Default for DanmuNotificationConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            danmu: true,
            super_chat: true,
            cooldown_secs: 2,
            expire_timeout_ms: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub cookies: Vec<AppCookie>,
    pub area_list: Value,
    pub theme: String,
    pub uid: u64,
    pub avatar: Option<String>,
    pub username: Option<String>,
    pub room_id: u64,
    pub csrf: Option<String>,
    pub room_title: String,
    pub category_id: String,
    pub area_id: String,
    pub img_url: String,
    pub sub_url: String,
    pub room_token: String,
    pub is_open_live: bool,
    pub streams: Vec<StreamCredential>,
    pub danmu_notifications: DanmuNotificationConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            cookies: Vec::new(),
            area_list: Value::Array(Vec::new()),
            theme: "light".to_string(),
            uid: 0,
            avatar: None,
            username: None,
            room_id: 0,
            csrf: None,
            room_title: String::new(),
            category_id: String::new(),
            area_id: String::new(),
            img_url: String::new(),
            sub_url: String::new(),
            room_token: String::new(),
            is_open_live: false,
            streams: Vec::new(),
            danmu_notifications: DanmuNotificationConfig::default(),
        }
    }
}

impl AppConfig {
    pub fn cookie_header(&self) -> String {
        self.cookies
            .iter()
            .filter(|cookie| !cookie.name.is_empty() && !cookie.value.is_empty())
            .map(|cookie| format!("{}={}", cookie.name, cookie.value))
            .collect::<Vec<_>>()
            .join("; ")
    }

    pub fn cookie(&self, name: &str) -> Option<&str> {
        self.cookies
            .iter()
            .find(|cookie| cookie.name == name)
            .map(|cookie| cookie.value.as_str())
    }

    pub fn set_cookies(&mut self, cookies: Vec<AppCookie>) {
        for cookie in cookies {
            if cookie.name.is_empty() {
                continue;
            }

            if let Some(existing) = self
                .cookies
                .iter_mut()
                .find(|existing| existing.name == cookie.name)
            {
                existing.value = cookie.value;
            } else {
                self.cookies.push(cookie);
            }
        }

        self.csrf = self.cookie("bili_jct").map(ToOwned::to_owned);
    }

    pub fn clear_auth(&mut self) {
        self.cookies.clear();
        self.csrf = None;
        self.uid = 0;
        self.avatar = None;
        self.username = None;
        self.room_id = 0;
        self.room_token.clear();
        self.streams.clear();
        self.is_open_live = false;
    }
}

#[derive(Clone)]
pub struct ConfigStore {
    path: Arc<PathBuf>,
    inner: Arc<Mutex<AppConfig>>,
}

impl ConfigStore {
    pub async fn load(path: Option<PathBuf>) -> std::io::Result<Self> {
        let explicit_path = path.is_some();
        let path = Arc::new(path.unwrap_or_else(default_config_path));
        let config = match read_config_if_exists(path.as_ref()).await? {
            Some(config) => config,
            None if explicit_path => AppConfig::default(),
            None => read_legacy_default_config().await?.unwrap_or_default(),
        };

        Ok(Self {
            path,
            inner: Arc::new(Mutex::new(config)),
        })
    }

    pub fn path(&self) -> &Path {
        self.path.as_ref()
    }

    pub async fn get(&self) -> AppConfig {
        self.inner.lock().await.clone()
    }

    pub async fn save(&self) -> std::io::Result<()> {
        let config = self.inner.lock().await.clone();
        self.save_config(&config).await
    }

    pub async fn update<F>(&self, update: F) -> std::io::Result<AppConfig>
    where
        F: FnOnce(&mut AppConfig),
    {
        let config = {
            let mut guard = self.inner.lock().await;
            update(&mut guard);
            guard.clone()
        };
        self.save_config(&config).await?;
        Ok(config)
    }

    async fn save_config(&self, config: &AppConfig) -> std::io::Result<()> {
        if let Some(parent) = self.path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let content = serde_json::to_vec_pretty(config).map_err(std::io::Error::other)?;
        tokio::fs::write(self.path.as_ref(), content).await
    }
}

async fn read_config_if_exists(path: &Path) -> std::io::Result<Option<AppConfig>> {
    if !path.exists() {
        return Ok(None);
    }

    let content = tokio::fs::read_to_string(path).await?;
    Ok(Some(serde_json::from_str(&content).unwrap_or_default()))
}

async fn read_legacy_default_config() -> std::io::Result<Option<AppConfig>> {
    read_config_if_exists(&legacy_default_config_path()).await
}

pub fn default_state_dir() -> PathBuf {
    if let Some(path) = env::var_os("BILIVE_STATE_DIR") {
        return PathBuf::from(path);
    }

    #[cfg(target_os = "windows")]
    {
        if let Some(path) = env::var_os("APPDATA") {
            return PathBuf::from(path).join("bilive");
        }
    }

    #[cfg(target_os = "macos")]
    {
        if let Some(home) = env::var_os("HOME") {
            return PathBuf::from(home)
                .join("Library")
                .join("Application Support")
                .join("bilive");
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        if let Some(path) = env::var_os("XDG_STATE_HOME") {
            return PathBuf::from(path).join("bilive");
        }

        if let Some(home) = env::var_os("HOME") {
            return PathBuf::from(home)
                .join(".local")
                .join("state")
                .join("bilive");
        }
    }

    PathBuf::from(".").join(".bilive")
}

pub fn default_config_path() -> PathBuf {
    default_config_dir().join("config")
}

fn legacy_default_config_path() -> PathBuf {
    default_state_dir().join("config.json")
}

pub fn default_config_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        if let Some(path) = non_empty_os_string(env::var_os("APPDATA")) {
            return PathBuf::from(path).join("bilive");
        }

        if let Some(home) = non_empty_os_string(env::var_os("USERPROFILE")) {
            return PathBuf::from(home)
                .join("AppData")
                .join("Roaming")
                .join("bilive");
        }

        PathBuf::from(".").join("bilive")
    }

    #[cfg(not(target_os = "windows"))]
    {
        default_config_dir_from_env(env::var_os("XDG_CONFIG_HOME"), env::var_os("HOME"))
    }
}

fn non_empty_os_string(value: Option<OsString>) -> Option<OsString> {
    value.filter(|value| !value.is_empty())
}

#[cfg(not(target_os = "windows"))]
fn default_config_dir_from_env(
    xdg_config_home: Option<OsString>,
    home: Option<OsString>,
) -> PathBuf {
    if let Some(path) = non_empty_os_string(xdg_config_home) {
        return PathBuf::from(path).join("bilive");
    }

    if let Some(home) = non_empty_os_string(home) {
        return PathBuf::from(home).join(".config").join("bilive");
    }

    PathBuf::from(".").join(".config").join("bilive")
}

pub fn parse_cookie_header(cookie_header: &str) -> Vec<AppCookie> {
    cookie_header
        .split(';')
        .filter_map(|part| {
            let (name, value) = part.trim().split_once('=')?;
            let name = name.trim();
            let value = value.trim();
            if name.is_empty() || value.is_empty() {
                return None;
            }
            Some(AppCookie {
                name: name.to_string(),
                value: value.to_string(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_config_path(name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir()
            .join(format!(
                "bilive-config-{name}-{}-{nanos}",
                std::process::id()
            ))
            .join("config.json")
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn default_config_path_uses_xdg_config_home() {
        let path =
            default_config_dir_from_env(Some("/tmp/bilive-config".into()), None).join("config");

        assert_eq!(path, PathBuf::from("/tmp/bilive-config/bilive/config"));
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn default_config_path_falls_back_to_home_config() {
        let path = default_config_dir_from_env(None, Some("/home/alice".into())).join("config");

        assert_eq!(path, PathBuf::from("/home/alice/.config/bilive/config"));
    }

    #[test]
    fn parses_browser_cookie_header() {
        let cookies = parse_cookie_header("SESSDATA=abc; bili_jct=csrf; empty=; theme=dark");
        assert_eq!(
            cookies,
            vec![
                AppCookie {
                    name: "SESSDATA".to_string(),
                    value: "abc".to_string(),
                },
                AppCookie {
                    name: "bili_jct".to_string(),
                    value: "csrf".to_string(),
                },
                AppCookie {
                    name: "theme".to_string(),
                    value: "dark".to_string(),
                },
            ]
        );
    }

    #[test]
    fn set_cookies_updates_csrf() {
        let mut config = AppConfig::default();
        config.set_cookies(vec![AppCookie {
            name: "bili_jct".to_string(),
            value: "csrf".to_string(),
        }]);
        assert_eq!(config.csrf.as_deref(), Some("csrf"));
    }

    #[test]
    fn cookie_header_filters_empty_names_and_values() {
        let config = AppConfig {
            cookies: vec![
                AppCookie {
                    name: "SESSDATA".to_string(),
                    value: "abc".to_string(),
                },
                AppCookie {
                    name: String::new(),
                    value: "ignored".to_string(),
                },
                AppCookie {
                    name: "empty".to_string(),
                    value: String::new(),
                },
                AppCookie {
                    name: "bili_jct".to_string(),
                    value: "csrf".to_string(),
                },
            ],
            ..Default::default()
        };

        assert_eq!(config.cookie_header(), "SESSDATA=abc; bili_jct=csrf");
    }

    #[test]
    fn set_cookies_replaces_existing_values_and_ignores_empty_names() {
        let mut config = AppConfig::default();
        config.set_cookies(vec![
            AppCookie {
                name: "SESSDATA".to_string(),
                value: "old".to_string(),
            },
            AppCookie {
                name: "bili_jct".to_string(),
                value: "old-csrf".to_string(),
            },
        ]);

        config.set_cookies(vec![
            AppCookie {
                name: "SESSDATA".to_string(),
                value: "new".to_string(),
            },
            AppCookie {
                name: String::new(),
                value: "ignored".to_string(),
            },
            AppCookie {
                name: "bili_jct".to_string(),
                value: "new-csrf".to_string(),
            },
        ]);

        assert_eq!(config.cookies.len(), 2);
        assert_eq!(config.cookie("SESSDATA"), Some("new"));
        assert_eq!(config.csrf.as_deref(), Some("new-csrf"));
    }

    #[test]
    fn clear_auth_removes_login_and_live_state() {
        let mut config = AppConfig {
            cookies: vec![AppCookie {
                name: "SESSDATA".to_string(),
                value: "abc".to_string(),
            }],
            csrf: Some("csrf".to_string()),
            uid: 123,
            avatar: Some("face".to_string()),
            username: Some("name".to_string()),
            room_id: 456,
            room_token: "token".to_string(),
            is_open_live: true,
            streams: vec![StreamCredential {
                kind: "rtmp-1".to_string(),
                address: "rtmp://example/live".to_string(),
                key: "secret".to_string(),
            }],
            ..Default::default()
        };

        config.clear_auth();

        assert!(config.cookies.is_empty());
        assert!(config.csrf.is_none());
        assert_eq!(config.uid, 0);
        assert!(config.avatar.is_none());
        assert!(config.username.is_none());
        assert_eq!(config.room_id, 0);
        assert!(config.room_token.is_empty());
        assert!(!config.is_open_live);
        assert!(config.streams.is_empty());
    }

    #[test]
    fn danmu_notifications_default_to_disabled() {
        let config: AppConfig = serde_json::from_str("{}").unwrap();

        assert!(!config.danmu_notifications.enabled);
        assert!(config.danmu_notifications.danmu);
        assert!(config.danmu_notifications.super_chat);
        assert_eq!(config.danmu_notifications.cooldown_secs, 2);
        assert_eq!(config.danmu_notifications.expire_timeout_ms, 0);
    }

    #[tokio::test]
    async fn config_store_loads_default_and_persists_updates() {
        let path = unique_config_path("persist");
        let parent = path.parent().unwrap().to_path_buf();

        let store = ConfigStore::load(Some(path.clone())).await.unwrap();
        assert_eq!(store.path(), path.as_path());
        assert_eq!(store.get().await.theme, "light");
        assert!(!path.exists());

        let updated = store
            .update(|config| {
                config.theme = "dark".to_string();
                config.room_id = 12345;
                config.set_cookies(vec![AppCookie {
                    name: "bili_jct".to_string(),
                    value: "csrf".to_string(),
                }]);
            })
            .await
            .unwrap();

        assert_eq!(updated.theme, "dark");
        assert_eq!(updated.room_id, 12345);
        assert_eq!(updated.csrf.as_deref(), Some("csrf"));
        assert!(path.exists());

        let reloaded = ConfigStore::load(Some(path.clone())).await.unwrap();
        let saved = reloaded.get().await;
        assert_eq!(saved.theme, "dark");
        assert_eq!(saved.room_id, 12345);
        assert_eq!(saved.csrf.as_deref(), Some("csrf"));

        let _ = tokio::fs::remove_dir_all(parent).await;
    }

    #[tokio::test]
    async fn config_store_falls_back_to_default_for_invalid_json() {
        let path = unique_config_path("invalid");
        let parent = path.parent().unwrap().to_path_buf();
        tokio::fs::create_dir_all(&parent).await.unwrap();
        tokio::fs::write(&path, b"{not json").await.unwrap();

        let store = ConfigStore::load(Some(path.clone())).await.unwrap();

        assert_eq!(store.get().await.theme, "light");
        let _ = tokio::fs::remove_dir_all(parent).await;
    }

    #[tokio::test]
    async fn explicit_config_path_ignores_legacy_default_config() {
        let path = unique_config_path("explicit");
        let parent = path.parent().unwrap().to_path_buf();

        let store = ConfigStore::load(Some(path)).await.unwrap();
        let config = store.get().await;

        assert_eq!(config.theme, AppConfig::default().theme);
        assert_eq!(config.room_id, 0);
        assert!(config.cookies.is_empty());
        let _ = tokio::fs::remove_dir_all(parent).await;
    }
}
