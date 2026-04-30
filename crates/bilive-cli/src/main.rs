// Copyright (C) 2026 Jamie Cui
// Author: Jamie Cui
// SPDX-License-Identifier: GPL-3.0-or-later

use anyhow::{Context, bail};
use clap::{Args, Parser, Subcommand};
use directories::ProjectDirs;
use std::{
    env,
    fs::{self, OpenOptions},
    net::SocketAddr,
    os::unix::process::CommandExt,
    path::PathBuf,
    process::{Command as ProcessCommand, Stdio},
    time::{Duration, Instant},
};
use tokio::time::sleep;
use tracing_subscriber::{EnvFilter, fmt};

const APP_QUALIFIER: &str = "com";
const APP_ORGANIZATION: &str = "bilive";
const APP_NAME: &str = "bilive";
const DEFAULT_LISTEN: &str = "127.0.0.1:22333";
const DEFAULT_WEB_DIR: &str = "web";
const PID_FILE_NAME: &str = "bilive.pid";
const LOG_FILE_NAME: &str = "bilive.log";
const DEFAULT_TIMEOUT_SECONDS: u64 = 10;

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
    /// Start bilive as a background service.
    Start(StartArgs),

    /// Stop the background service.
    Stop(ControlArgs),

    /// Show background service status.
    Status(StatusArgs),

    /// Restart the background service.
    Restart(StartArgs),

    /// Run bilive in the foreground.
    #[command(hide = true)]
    Serve(ServiceArgs),
}

#[derive(Debug, Args, Clone)]
struct ServiceArgs {
    #[arg(long, env = "BILIVE_LISTEN", default_value = DEFAULT_LISTEN)]
    listen: SocketAddr,

    #[arg(long, env = "BILIVE_WEB_DIR", default_value = DEFAULT_WEB_DIR)]
    web_dir: PathBuf,

    #[arg(long, env = "BILIVE_CONFIG")]
    config: Option<PathBuf>,
}

#[derive(Debug, Args, Clone)]
struct StartArgs {
    #[command(flatten)]
    service: ServiceArgs,

    #[command(flatten)]
    control: ControlArgs,
}

#[derive(Debug, Args, Clone)]
struct StatusArgs {
    #[arg(long, env = "BILIVE_LISTEN", default_value = DEFAULT_LISTEN)]
    listen: SocketAddr,

    #[command(flatten)]
    control: ControlArgs,
}

#[derive(Debug, Args, Clone)]
struct ControlArgs {
    #[arg(long, env = "BILIVE_STATE_DIR")]
    state_dir: Option<PathBuf>,

    #[arg(long)]
    pid_file: Option<PathBuf>,

    #[arg(long)]
    log_file: Option<PathBuf>,

    #[arg(long, default_value_t = DEFAULT_TIMEOUT_SECONDS)]
    timeout: u64,
}

#[derive(Debug, Clone)]
struct RuntimePaths {
    state_dir: PathBuf,
    pid_file: PathBuf,
    log_file: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ServiceState {
    Running,
    StalePid,
    Stopped,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_logging();

    match Cli::parse().command {
        Command::Start(args) => start(args).await,
        Command::Stop(args) => stop(args).await,
        Command::Status(args) => status(args).await,
        Command::Restart(args) => restart(args).await,
        Command::Serve(args) => serve(args).await,
    }
}

async fn serve(args: ServiceArgs) -> anyhow::Result<()> {
    let web_dir = resolve_web_dir(args.web_dir)?;
    bilive_server::run(bilive_server::ServerConfig {
        listen: args.listen,
        web_dir,
        config_path: args.config,
    })
    .await
}

async fn start(args: StartArgs) -> anyhow::Result<()> {
    let paths = runtime_paths(&args.control)?;
    fs::create_dir_all(&paths.state_dir)
        .with_context(|| format!("failed to create state dir {}", paths.state_dir.display()))?;

    match service_state(&paths)? {
        ServiceState::Running => {
            let pid = read_pid(&paths.pid_file)?;
            println!("bilive is already running (pid {pid})");
            return Ok(());
        }
        ServiceState::StalePid => {
            fs::remove_file(&paths.pid_file).with_context(|| {
                format!(
                    "failed to remove stale pid file {}",
                    paths.pid_file.display()
                )
            })?;
        }
        ServiceState::Stopped => {}
    }

    let web_dir = resolve_web_dir(args.service.web_dir.clone())?;

    let log = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&paths.log_file)
        .with_context(|| format!("failed to open log file {}", paths.log_file.display()))?;
    let log_for_stderr = log
        .try_clone()
        .with_context(|| format!("failed to clone log file {}", paths.log_file.display()))?;

    let mut command =
        ProcessCommand::new(env::current_exe().context("failed to resolve current executable")?);
    command
        .arg("serve")
        .arg("--listen")
        .arg(args.service.listen.to_string())
        .arg("--web-dir")
        .arg(&web_dir)
        .env("BILIVE_STATE_DIR", &paths.state_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(log_for_stderr));

    if let Some(config) = &args.service.config {
        command.arg("--config").arg(config);
    }

    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                Err(std::io::Error::last_os_error())
            } else {
                Ok(())
            }
        });
    }

    let child = command.spawn().context("failed to start bilive service")?;
    let pid = child.id();
    fs::write(&paths.pid_file, format!("{pid}\n"))
        .with_context(|| format!("failed to write pid file {}", paths.pid_file.display()))?;

    if wait_for_health(
        args.service.listen,
        Duration::from_secs(args.control.timeout),
    )
    .await
    {
        println!("started bilive at http://{}", args.service.listen);
    } else {
        println!("started bilive (pid {pid}), but health check did not pass before timeout");
    }
    println!("pid: {pid}");
    println!("log: {}", paths.log_file.display());
    Ok(())
}

async fn stop(args: ControlArgs) -> anyhow::Result<()> {
    let paths = runtime_paths(&args)?;
    match service_state(&paths)? {
        ServiceState::Stopped => {
            println!("bilive is not running");
            Ok(())
        }
        ServiceState::StalePid => {
            fs::remove_file(&paths.pid_file).with_context(|| {
                format!(
                    "failed to remove stale pid file {}",
                    paths.pid_file.display()
                )
            })?;
            println!("bilive is not running; removed stale pid file");
            Ok(())
        }
        ServiceState::Running => {
            let pid = read_pid(&paths.pid_file)?;
            terminate(pid)?;
            if wait_for_exit(pid, Duration::from_secs(args.timeout)).await {
                fs::remove_file(&paths.pid_file).with_context(|| {
                    format!("failed to remove pid file {}", paths.pid_file.display())
                })?;
                println!("stopped bilive (pid {pid})");
                Ok(())
            } else {
                bail!("timed out waiting for bilive to stop (pid {pid})")
            }
        }
    }
}

async fn status(args: StatusArgs) -> anyhow::Result<()> {
    let paths = runtime_paths(&args.control)?;
    match service_state(&paths)? {
        ServiceState::Stopped => {
            println!("bilive is stopped");
            Ok(())
        }
        ServiceState::StalePid => {
            let pid = read_pid(&paths.pid_file)?;
            println!("bilive is stopped (stale pid file for pid {pid})");
            println!("pid file: {}", paths.pid_file.display());
            Ok(())
        }
        ServiceState::Running => {
            let pid = read_pid(&paths.pid_file)?;
            if health_is_ok(args.listen).await {
                println!("bilive is running and healthy (pid {pid})");
            } else {
                println!("bilive is running but health check failed (pid {pid})");
            }
            println!("url: http://{}", args.listen);
            println!("pid file: {}", paths.pid_file.display());
            println!("log: {}", paths.log_file.display());
            Ok(())
        }
    }
}

async fn restart(args: StartArgs) -> anyhow::Result<()> {
    stop(args.control.clone()).await?;
    start(args).await
}

fn runtime_paths(args: &ControlArgs) -> anyhow::Result<RuntimePaths> {
    let state_dir = match &args.state_dir {
        Some(path) => path.clone(),
        None => default_state_dir()?,
    };
    let pid_file = args
        .pid_file
        .clone()
        .unwrap_or_else(|| state_dir.join(PID_FILE_NAME));
    let log_file = args
        .log_file
        .clone()
        .unwrap_or_else(|| state_dir.join(LOG_FILE_NAME));

    Ok(RuntimePaths {
        state_dir,
        pid_file,
        log_file,
    })
}

fn resolve_web_dir(web_dir: PathBuf) -> anyhow::Result<PathBuf> {
    let web_dir = if web_dir.is_absolute() {
        web_dir
    } else {
        env::current_dir()
            .context("failed to resolve current directory")?
            .join(web_dir)
    };
    let index = web_dir.join("index.html");
    if !index.is_file() {
        bail!(
            "web dir {} does not contain index.html; pass --web-dir with the static UI directory",
            web_dir.display()
        );
    }
    fs::canonicalize(&web_dir)
        .with_context(|| format!("failed to resolve web dir {}", web_dir.display()))
}

fn default_state_dir() -> anyhow::Result<PathBuf> {
    if let Some(path) = env::var_os("BILIVE_STATE_DIR") {
        return Ok(PathBuf::from(path));
    }

    let dirs = ProjectDirs::from(APP_QUALIFIER, APP_ORGANIZATION, APP_NAME)
        .context("failed to determine platform state directory")?;
    Ok(dirs
        .state_dir()
        .unwrap_or_else(|| dirs.data_local_dir())
        .to_path_buf())
}

fn service_state(paths: &RuntimePaths) -> anyhow::Result<ServiceState> {
    if !paths.pid_file.exists() {
        return Ok(ServiceState::Stopped);
    }

    let pid = read_pid(&paths.pid_file)?;
    if process_exists(pid)? {
        Ok(ServiceState::Running)
    } else {
        Ok(ServiceState::StalePid)
    }
}

fn read_pid(path: &PathBuf) -> anyhow::Result<u32> {
    let contents = fs::read_to_string(path)
        .with_context(|| format!("failed to read pid file {}", path.display()))?;
    contents
        .trim()
        .parse::<u32>()
        .with_context(|| format!("invalid pid file {}", path.display()))
}

fn process_exists(pid: u32) -> anyhow::Result<bool> {
    let result = unsafe { libc::kill(pid as libc::pid_t, 0) };
    if result == 0 {
        return Ok(true);
    }

    let error = std::io::Error::last_os_error();
    match error.raw_os_error() {
        Some(code) if code == libc::ESRCH => Ok(false),
        Some(code) if code == libc::EPERM => Ok(true),
        _ => Err(error).with_context(|| format!("failed to inspect process {pid}")),
    }
}

fn terminate(pid: u32) -> anyhow::Result<()> {
    let result = unsafe { libc::kill(pid as libc::pid_t, libc::SIGTERM) };
    if result == 0 {
        return Ok(());
    }

    let error = std::io::Error::last_os_error();
    match error.raw_os_error() {
        Some(code) if code == libc::ESRCH => Ok(()),
        _ => Err(error).with_context(|| format!("failed to terminate process {pid}")),
    }
}

async fn wait_for_exit(pid: u32, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        match process_exists(pid) {
            Ok(false) => return true,
            Ok(true) => {}
            Err(_) => return false,
        }

        if Instant::now() >= deadline {
            return false;
        }
        sleep(Duration::from_millis(100)).await;
    }
}

async fn wait_for_health(listen: SocketAddr, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;

    loop {
        if health_is_ok(listen).await {
            return true;
        }

        if Instant::now() >= deadline {
            return false;
        }
        sleep(Duration::from_millis(100)).await;
    }
}

async fn health_is_ok(listen: SocketAddr) -> bool {
    let url = format!("http://{listen}/api/health");
    match reqwest::get(url).await {
        Ok(response) => response.status().is_success(),
        Err(_) => false,
    }
}

fn init_logging() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        EnvFilter::new("bilive=info,bilive_server=info,bilive_core=info,tower_http=info")
    });

    fmt().with_env_filter(filter).init();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_dir(name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("bilive-cli-{name}-{}-{nanos}", std::process::id()))
    }

    fn control_args(state_dir: PathBuf) -> ControlArgs {
        ControlArgs {
            state_dir: Some(state_dir),
            pid_file: None,
            log_file: None,
            timeout: 1,
        }
    }

    #[test]
    fn runtime_paths_default_files_live_under_state_dir() {
        let state_dir = unique_dir("runtime-defaults");
        let paths = runtime_paths(&control_args(state_dir.clone())).unwrap();

        assert_eq!(paths.state_dir, state_dir);
        assert_eq!(paths.pid_file, paths.state_dir.join(PID_FILE_NAME));
        assert_eq!(paths.log_file, paths.state_dir.join(LOG_FILE_NAME));
    }

    #[test]
    fn runtime_paths_honors_file_overrides() {
        let state_dir = unique_dir("runtime-overrides");
        let pid_file = state_dir.join("custom.pid");
        let log_file = state_dir.join("custom.log");
        let args = ControlArgs {
            state_dir: Some(state_dir.clone()),
            pid_file: Some(pid_file.clone()),
            log_file: Some(log_file.clone()),
            timeout: 1,
        };

        let paths = runtime_paths(&args).unwrap();

        assert_eq!(paths.state_dir, state_dir);
        assert_eq!(paths.pid_file, pid_file);
        assert_eq!(paths.log_file, log_file);
    }

    #[test]
    fn read_pid_trims_whitespace_and_rejects_invalid_files() {
        let dir = unique_dir("pid");
        fs::create_dir_all(&dir).unwrap();
        let pid_file = dir.join("bilive.pid");

        fs::write(&pid_file, " 1234\n").unwrap();
        assert_eq!(read_pid(&pid_file).unwrap(), 1234);

        fs::write(&pid_file, "not-a-pid").unwrap();
        assert!(read_pid(&pid_file).is_err());

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn service_state_detects_stopped_running_and_stale_pid_files() {
        let dir = unique_dir("state");
        fs::create_dir_all(&dir).unwrap();
        let paths = RuntimePaths {
            state_dir: dir.clone(),
            pid_file: dir.join("bilive.pid"),
            log_file: dir.join("bilive.log"),
        };

        assert_eq!(service_state(&paths).unwrap(), ServiceState::Stopped);

        fs::write(&paths.pid_file, format!("{}\n", std::process::id())).unwrap();
        assert_eq!(service_state(&paths).unwrap(), ServiceState::Running);

        let mut child = std::process::Command::new("sh")
            .arg("-c")
            .arg("exit 0")
            .spawn()
            .unwrap();
        let stale_pid = child.id();
        child.wait().unwrap();
        fs::write(&paths.pid_file, format!("{stale_pid}\n")).unwrap();
        assert_eq!(service_state(&paths).unwrap(), ServiceState::StalePid);

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn process_exists_detects_current_process() {
        assert!(process_exists(std::process::id()).unwrap());
    }
}
