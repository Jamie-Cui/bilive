// Copyright (C) 2026 Jamie Cui
// Author: Jamie Cui
// SPDX-License-Identifier: GPL-3.0-or-later

use clap::{Parser, Subcommand};
use std::{net::SocketAddr, path::PathBuf};
use tracing_subscriber::{EnvFilter, fmt};

#[derive(Debug, Parser)]
#[command(
    name = "bilive",
    version,
    about = "Local Bilibili live management service"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Serve {
        #[arg(long, env = "BILIVE_LISTEN", default_value = "127.0.0.1:22333")]
        listen: SocketAddr,

        #[arg(long, env = "BILIVE_WEB_DIR", default_value = "web")]
        web_dir: PathBuf,

        #[arg(long, env = "BILIVE_CONFIG")]
        config: Option<PathBuf>,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_logging();

    match Cli::parse().command {
        Command::Serve {
            listen,
            web_dir,
            config,
        } => {
            bilive_server::run(bilive_server::ServerConfig {
                listen,
                web_dir,
                config_path: config,
            })
            .await
        }
    }
}

fn init_logging() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        EnvFilter::new("bilive=info,bilive_server=info,bilive_core=info,tower_http=info")
    });

    fmt().with_env_filter(filter).init();
}
