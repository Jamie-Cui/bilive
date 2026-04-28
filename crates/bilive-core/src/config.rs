// Copyright (C) 2026 Jamie Cui
// Author: Jamie Cui
// SPDX-License-Identifier: GPL-3.0-or-later

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    env,
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
        let path = Arc::new(path.unwrap_or_else(default_config_path));
        let config = if path.exists() {
            let content = tokio::fs::read_to_string(path.as_ref()).await?;
            serde_json::from_str(&content).unwrap_or_default()
        } else {
            AppConfig::default()
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
    default_state_dir().join("config.json")
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
}
