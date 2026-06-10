// Copyright (C) 2026 Jamie Cui
// Author: Jamie Cui
// SPDX-License-Identifier: GPL-3.0-or-later

mod cli;
mod model;
mod overlay;
mod service;

use clap::Parser;
use cli::Cli;
use model::OverlayCommand;
use overlay::{FontSizeAction, OverlayConfig};
use service::{ServiceUrl, spawn_test_messages, start_live_messages};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tokio::sync::mpsc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Cli::parse();
    let config = OverlayConfig::from_cli(&args)?;
    let (tx, rx) = mpsc::unbounded_channel();
    let (command_tx, command_rx) = mpsc::unbounded_channel::<OverlayCommand>();
    let (font_tx, font_rx) = mpsc::unbounded_channel();
    let shutdown = Arc::new(AtomicBool::new(false));

    if args.test_overlay {
        spawn_test_messages(tx.clone());
    } else {
        start_live_messages(
            ServiceUrl::new(&args.url),
            args.room_id,
            args.no_connect,
            args.show_system,
            tx.clone(),
            command_rx,
        )
        .await?;
    }

    let signal_shutdown = Arc::clone(&shutdown);
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            signal_shutdown.store(true, Ordering::Relaxed);
        }
    });

    #[cfg(unix)]
    spawn_font_size_signal_handlers(font_tx)?;

    let result = overlay::run(config, rx, font_rx, command_tx, Arc::clone(&shutdown));
    shutdown.store(true, Ordering::Relaxed);
    drop(tx);
    result
}

#[cfg(unix)]
fn spawn_font_size_signal_handlers(
    tx: mpsc::UnboundedSender<FontSizeAction>,
) -> anyhow::Result<()> {
    use tokio::signal::unix::{SignalKind, signal};

    let mut increase = signal(SignalKind::user_defined1())?;
    let mut decrease = signal(SignalKind::user_defined2())?;
    tokio::spawn(async move {
        loop {
            tokio::select! {
                value = increase.recv() => {
                    if value.is_none() || tx.send(FontSizeAction::Increase).is_err() {
                        break;
                    }
                }
                value = decrease.recv() => {
                    if value.is_none() || tx.send(FontSizeAction::Decrease).is_err() {
                        break;
                    }
                }
            }
        }
    });
    Ok(())
}
