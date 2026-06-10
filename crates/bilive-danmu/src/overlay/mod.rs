// Copyright (C) 2026 Jamie Cui
// Author: Jamie Cui
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::{
    cli::{Backend, Cli},
    model::{OverlayCommand, OverlayEvent, OverlayMessage},
};
use anyhow::{bail, ensure};
use std::{
    collections::HashSet,
    sync::{Arc, atomic::AtomicBool},
    time::Duration,
};
use tokio::sync::mpsc;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "linux")]
mod x11;

const DEFAULT_FRAME_MILLIS: u64 = 16;
const FONT_SIZE_STEP: f64 = 2.0;
const MIN_FONT_SIZE: f64 = 12.0;
const MAX_FONT_SIZE: f64 = 96.0;
const HISTORY_LIMIT: usize = 1_000;
pub const WHEEL_SCROLL_LINES: usize = 3;

#[derive(Debug, Clone)]
pub struct OverlayConfig {
    pub backend: Backend,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub height_ratio: f32,
    pub overlay: bool,
    pub font_family: String,
    pub font_size: f64,
    pub max_lines: usize,
    pub opacity: f64,
    pub click_through: bool,
}

impl OverlayConfig {
    pub fn from_cli(args: &Cli) -> anyhow::Result<Self> {
        ensure!(args.font_size > 0.0, "--font-size must be greater than 0");
        ensure!(args.max_lines > 0, "--max-lines must be greater than 0");
        ensure!(
            (0.05..=1.0).contains(&args.height_ratio),
            "--height-ratio must be between 0.05 and 1.0"
        );
        ensure!(
            (0.0..=1.0).contains(&args.opacity),
            "--opacity must be between 0.0 and 1.0"
        );

        Ok(Self {
            backend: args.backend,
            x: args.x,
            y: args.y,
            width: args.width,
            height: args.height,
            height_ratio: args.height_ratio,
            overlay: args.overlay,
            font_family: args.font_family.clone(),
            font_size: args.font_size,
            max_lines: args.max_lines,
            opacity: args.opacity,
            click_through: args.overlay && !args.no_click_through,
        })
    }
}

#[derive(Debug, Clone)]
pub struct FrameText {
    pub text: String,
    pub x: f64,
    pub y: f64,
}

#[derive(Debug)]
pub struct OverlayState {
    messages: Vec<OverlayMessage>,
    seen: HashSet<String>,
    font_size: f64,
    height: f64,
    max_lines: usize,
    scroll_offset: usize,
    changed: bool,
}

impl OverlayState {
    pub fn new(width: u32, height: u32, config: &OverlayConfig) -> Self {
        let _ = width;
        let line_height = (config.font_size * 1.45).max(24.0);
        let visible_lines = ((height as f64) / line_height).floor().max(1.0) as usize;

        Self {
            messages: Vec::new(),
            seen: HashSet::new(),
            font_size: config.font_size,
            height: height as f64,
            max_lines: visible_lines.min(config.max_lines).max(1),
            scroll_offset: 0,
            changed: true,
        }
    }

    pub fn resize(&mut self, width: u32, height: u32, config: &OverlayConfig) {
        let _ = width;
        self.height = height as f64;
        let line_height = (config.font_size * 1.45).max(24.0);
        let visible_lines = ((height as f64) / line_height).floor().max(1.0) as usize;
        self.max_lines = visible_lines.min(config.max_lines).max(1);
        self.clamp_scroll_offset();
        self.changed = true;
    }

    pub fn set_font_size(&mut self, font_size: f64, config: &OverlayConfig) {
        self.font_size = font_size;
        let line_height = (font_size * 1.45).max(24.0);
        let visible_lines = (self.height / line_height).floor().max(1.0) as usize;
        self.max_lines = visible_lines.min(config.max_lines).max(1);
        self.clamp_scroll_offset();
        self.changed = true;
    }

    pub fn push(&mut self, message: OverlayMessage) {
        if !self.seen.insert(message.id.clone()) {
            return;
        }
        let was_scrolled = self.scroll_offset > 0;
        self.messages.push(message);
        self.messages.sort_by(compare_messages);
        if was_scrolled {
            self.scroll_offset = self.scroll_offset.saturating_add(1);
        }
        self.trim_history();
        self.clamp_scroll_offset();
        self.changed = true;
    }

    pub fn replace(&mut self, messages: Vec<OverlayMessage>) {
        self.messages.clear();
        self.seen.clear();
        self.scroll_offset = 0;

        for message in messages {
            if self.seen.insert(message.id.clone()) {
                self.messages.push(message);
            }
        }
        self.messages.sort_by(compare_messages);
        self.trim_history();
        self.changed = true;
    }

    pub fn apply_event(&mut self, event: OverlayEvent) {
        match event {
            OverlayEvent::Push(message) => self.push(message),
            OverlayEvent::Replace(messages) => self.replace(messages),
        }
    }

    pub fn scroll_lines(&mut self, lines: isize) -> bool {
        let next = if lines.is_positive() {
            self.scroll_offset.saturating_add(lines as usize)
        } else {
            self.scroll_offset.saturating_sub(lines.unsigned_abs())
        }
        .min(self.max_scroll_offset());

        if next == self.scroll_offset {
            return false;
        }

        self.scroll_offset = next;
        self.changed = true;
        true
    }

    pub fn tick(&mut self) -> bool {
        let changed = self.changed;
        self.changed = false;
        changed
    }

    pub fn frame_texts(&self) -> Vec<FrameText> {
        let end = self.messages.len().saturating_sub(self.scroll_offset);
        let start = end.saturating_sub(self.max_lines);
        self.messages[start..end]
            .iter()
            .enumerate()
            .map(|(index, message)| FrameText {
                text: message.display_text(),
                x: self.font_size * 0.75,
                y: chat_line_y(index, self.font_size, self.height),
            })
            .collect()
    }

    fn trim_history(&mut self) {
        if self.messages.len() <= HISTORY_LIMIT {
            return;
        }
        let excess = self.messages.len() - HISTORY_LIMIT;
        self.messages.drain(0..excess);
    }

    fn max_scroll_offset(&self) -> usize {
        self.messages.len().saturating_sub(self.max_lines)
    }

    fn clamp_scroll_offset(&mut self) {
        self.scroll_offset = self.scroll_offset.min(self.max_scroll_offset());
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontSizeAction {
    Increase,
    Decrease,
}

pub fn adjust_font_size(
    config: &mut OverlayConfig,
    state: &mut OverlayState,
    action: FontSizeAction,
) -> bool {
    let delta = match action {
        FontSizeAction::Increase => FONT_SIZE_STEP,
        FontSizeAction::Decrease => -FONT_SIZE_STEP,
    };
    let next = (config.font_size + delta).clamp(MIN_FONT_SIZE, MAX_FONT_SIZE);
    if (next - config.font_size).abs() < f64::EPSILON {
        return false;
    }

    config.font_size = next;
    state.set_font_size(next, config);
    true
}

fn chat_line_y(index: usize, font_size: f64, height: f64) -> f64 {
    let line_height = (font_size * 1.45).max(24.0);
    ((index as f64) * line_height + font_size * 1.15).min(height - font_size * 0.25)
}

fn compare_messages(left: &OverlayMessage, right: &OverlayMessage) -> std::cmp::Ordering {
    left.received_at
        .cmp(&right.received_at)
        .then_with(|| left.sequence.cmp(&right.sequence))
        .then_with(|| left.id.cmp(&right.id))
}

pub fn frame_duration() -> Duration {
    Duration::from_millis(DEFAULT_FRAME_MILLIS)
}

pub fn run(
    config: OverlayConfig,
    rx: mpsc::UnboundedReceiver<OverlayEvent>,
    font_rx: mpsc::UnboundedReceiver<FontSizeAction>,
    command_tx: mpsc::UnboundedSender<OverlayCommand>,
    shutdown: Arc<AtomicBool>,
) -> anyhow::Result<()> {
    match config.backend {
        Backend::Auto => run_auto(config, rx, font_rx, command_tx, shutdown),
        Backend::X11 => run_x11(config, rx, font_rx, command_tx, shutdown),
        Backend::Macos => run_macos(config, rx, font_rx, command_tx, shutdown),
    }
}

fn run_auto(
    config: OverlayConfig,
    rx: mpsc::UnboundedReceiver<OverlayEvent>,
    font_rx: mpsc::UnboundedReceiver<FontSizeAction>,
    command_tx: mpsc::UnboundedSender<OverlayCommand>,
    shutdown: Arc<AtomicBool>,
) -> anyhow::Result<()> {
    #[cfg(target_os = "macos")]
    {
        return run_macos(config, rx, font_rx, command_tx, shutdown);
    }

    #[cfg(target_os = "linux")]
    {
        return run_x11(config, rx, font_rx, command_tx, shutdown);
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = (config, rx, font_rx, command_tx, shutdown);
        bail!("bilive-danmu overlay currently supports Linux X11/i3 and macOS");
    }
}

fn run_x11(
    config: OverlayConfig,
    rx: mpsc::UnboundedReceiver<OverlayEvent>,
    font_rx: mpsc::UnboundedReceiver<FontSizeAction>,
    command_tx: mpsc::UnboundedSender<OverlayCommand>,
    shutdown: Arc<AtomicBool>,
) -> anyhow::Result<()> {
    #[cfg(target_os = "linux")]
    {
        x11::run(config, rx, font_rx, command_tx, shutdown)
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = (config, rx, font_rx, command_tx, shutdown);
        bail!("--backend x11 is only available on Linux");
    }
}

fn run_macos(
    config: OverlayConfig,
    rx: mpsc::UnboundedReceiver<OverlayEvent>,
    font_rx: mpsc::UnboundedReceiver<FontSizeAction>,
    command_tx: mpsc::UnboundedSender<OverlayCommand>,
    shutdown: Arc<AtomicBool>,
) -> anyhow::Result<()> {
    #[cfg(target_os = "macos")]
    {
        macos::run(config, rx, font_rx, command_tx, shutdown)
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = (config, rx, font_rx, command_tx, shutdown);
        bail!("--backend macos is only available on macOS");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::LineKind;

    fn test_config(max_lines: usize) -> OverlayConfig {
        OverlayConfig {
            backend: Backend::Auto,
            x: 0,
            y: 0,
            width: 640,
            height: 120,
            height_ratio: 0.3,
            overlay: false,
            font_family: "sans".to_string(),
            font_size: 24.0,
            max_lines,
            opacity: 1.0,
            click_through: true,
        }
    }

    fn chat_message(id: u64) -> OverlayMessage {
        OverlayMessage {
            id: format!("id-{id}"),
            sequence: id,
            text: format!("message-{id}"),
            kind: LineKind::Chat,
            received_at: id,
        }
    }

    #[test]
    fn state_exposes_inserted_message_after_tick() {
        let config = test_config(4);
        let mut state = OverlayState::new(640, 120, &config);
        state.push(OverlayMessage::system(
            "id".to_string(),
            0,
            "hello",
            LineKind::System,
        ));

        assert!(state.tick());
        assert_eq!(state.frame_texts().len(), 1);
    }

    #[test]
    fn chat_state_orders_messages_by_time_then_sequence() {
        let config = test_config(4);
        let mut state = OverlayState::new(640, 120, &config);
        state.push(OverlayMessage {
            id: "late".to_string(),
            sequence: 0,
            text: "late".to_string(),
            kind: LineKind::Chat,
            received_at: 20_000,
        });
        state.push(OverlayMessage {
            id: "early".to_string(),
            sequence: 1,
            text: "early".to_string(),
            kind: LineKind::Chat,
            received_at: 10_000,
        });

        let texts = state.frame_texts();
        assert!(texts[0].text.ends_with("early"));
        assert!(texts[1].text.ends_with("late"));
    }

    #[test]
    fn font_size_adjustment_updates_chat_layout() {
        let mut config = test_config(10);
        let mut state = OverlayState::new(640, 120, &config);

        assert!(adjust_font_size(
            &mut config,
            &mut state,
            FontSizeAction::Increase
        ));
        assert_eq!(config.font_size, 26.0);
        assert!(state.tick());

        assert!(adjust_font_size(
            &mut config,
            &mut state,
            FontSizeAction::Decrease
        ));
        assert_eq!(config.font_size, 24.0);
    }

    #[test]
    fn scroll_lines_moves_between_history_and_latest_messages() {
        let config = test_config(3);
        let mut state = OverlayState::new(640, 120, &config);
        for id in 0..5 {
            state.push(chat_message(id));
        }

        let latest = state.frame_texts();
        assert!(latest[0].text.ends_with("message-2"));
        assert!(latest[2].text.ends_with("message-4"));

        assert!(state.scroll_lines(2));
        let history = state.frame_texts();
        assert!(history[0].text.ends_with("message-0"));
        assert!(history[2].text.ends_with("message-2"));

        assert!(state.scroll_lines(-2));
        let latest = state.frame_texts();
        assert!(latest[0].text.ends_with("message-2"));
        assert!(latest[2].text.ends_with("message-4"));
    }

    #[test]
    fn new_messages_do_not_force_scrolled_history_to_bottom() {
        let config = test_config(3);
        let mut state = OverlayState::new(640, 120, &config);
        for id in 0..5 {
            state.push(chat_message(id));
        }

        assert!(state.scroll_lines(2));
        state.push(chat_message(5));

        let visible = state.frame_texts();
        assert!(visible[0].text.ends_with("message-0"));
        assert!(visible[2].text.ends_with("message-2"));
    }

    #[test]
    fn replace_clears_old_messages_and_returns_to_latest() {
        let config = test_config(2);
        let mut state = OverlayState::new(640, 120, &config);
        for id in 0..4 {
            state.push(chat_message(id));
        }
        assert!(state.scroll_lines(2));

        state.replace(vec![chat_message(10), chat_message(11), chat_message(12)]);

        let visible = state.frame_texts();
        assert_eq!(visible.len(), 2);
        assert!(visible[0].text.ends_with("message-11"));
        assert!(visible[1].text.ends_with("message-12"));
        assert!(!visible.iter().any(|text| text.text.ends_with("message-0")));
    }
}
