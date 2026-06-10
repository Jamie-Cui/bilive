// Copyright (C) 2026 Jamie Cui
// Author: Jamie Cui
// SPDX-License-Identifier: GPL-3.0-or-later

use clap::{Parser, ValueEnum};

pub const DEFAULT_URL: &str = "http://127.0.0.1:22333";

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Backend {
    Auto,
    X11,
    Macos,
}

#[derive(Debug, Parser)]
#[command(
    name = "bilive-danmu",
    version,
    about = "Desktop danmu overlay for a running bilive service."
)]
pub struct Cli {
    /// Base URL for the local bilive service.
    #[arg(long, env = "BILIVE_URL", default_value = DEFAULT_URL)]
    pub url: String,

    /// Override the room id used when connecting danmu.
    #[arg(long)]
    pub room_id: Option<u64>,

    /// Window backend to use.
    #[arg(long, value_enum, default_value_t = Backend::Auto)]
    pub backend: Backend,

    /// Run as a click-through always-on-top overlay instead of a normal window.
    #[arg(long)]
    pub overlay: bool,

    /// Do not request /api/danmu/connect on startup.
    #[arg(long)]
    pub no_connect: bool,

    /// Show synthetic test messages without connecting to the bilive service.
    #[arg(long)]
    pub test_overlay: bool,

    /// Include service/system messages in the overlay.
    #[arg(long)]
    pub show_system: bool,

    /// Screen-relative x coordinate in pixels.
    #[arg(long, default_value_t = 0)]
    pub x: i32,

    /// Screen-relative y coordinate in pixels.
    #[arg(long, default_value_t = 0)]
    pub y: i32,

    /// Overlay width in pixels. Use 0 for the primary screen width.
    #[arg(long, default_value_t = 0)]
    pub width: u32,

    /// Overlay height in pixels. Use 0 with --height-ratio.
    #[arg(long, default_value_t = 0)]
    pub height: u32,

    /// Overlay height as a fraction of the primary screen when --height is 0.
    #[arg(long, default_value_t = 0.32)]
    pub height_ratio: f32,

    /// Font family passed to the platform text renderer.
    #[arg(long, default_value = "sans")]
    pub font_family: String,

    /// Font size in pixels.
    #[arg(long, default_value_t = 30.0)]
    pub font_size: f64,

    /// Maximum number of visible chat lines.
    #[arg(long = "max-lines", alias = "max-lanes", default_value_t = 10)]
    pub max_lines: usize,

    /// Message opacity from 0.0 to 1.0.
    #[arg(long, default_value_t = 0.95)]
    pub opacity: f64,

    /// Keep the overlay receiving mouse input for debugging. Only applies with --overlay.
    #[arg(long)]
    pub no_click_through: bool,
}
