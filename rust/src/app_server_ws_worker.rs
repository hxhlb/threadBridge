use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use anyhow::{Context, Result, anyhow, bail};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::process::Command;
use tokio_tungstenite::accept_async;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message as WsMessage;
use tracing::{debug, warn};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerReadyState {
    pub worker_ws_url: String,
    pub daemon_ws_url: String,
}

pub fn run_from_env() -> Result<()> {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("failed to build worker runtime")?;
    runtime.block_on(run_cli(args))
}

async fn run_cli(args: Vec<OsString>) -> Result<()> {
    let config = WorkerCli::parse(&args)?;
    run_worker(config).await
}

#[derive(Debug)]
struct WorkerCli {
    workspace: PathBuf,
    listen_ws_url: String,
    ready_file: PathBuf,
}

impl WorkerCli {
    fn parse(args: &[OsString]) -> Result<Self> {
        let mut workspace: Option<PathBuf> = None;
        let mut listen_ws_url: Option<String> = None;
        let mut ready_file: Option<PathBuf> = None;
        let mut iter = args.iter();
        while let Some(flag) = iter.next() {
            let flag = flag
                .to_str()
                .ok_or_else(|| anyhow!("worker arguments must be valid utf-8"))?;
            match flag {
                "--workspace" => {
                    let value = iter.next().context("missing value for --workspace")?;
                    workspace = Some(PathBuf::from(value));
                }
                "--listen-ws-url" => {
                    let value = iter
                        .next()
                        .context("missing value for --listen-ws-url")?
                        .to_str()
                        .context("--listen-ws-url must be valid utf-8")?;
                    listen_ws_url = Some(value.to_owned());
                }
                "--ready-file" => {
                    let value = iter.next().context("missing value for --ready-file")?;
                    ready_file = Some(PathBuf::from(value));
                }
                other => bail!("unsupported app_server_ws_worker argument: {other}"),
            }
        }

        Ok(Self {
            workspace: workspace.context("missing required --workspace")?,
            listen_ws_url: listen_ws_url.context("missing required --listen-ws-url")?,
            ready_file: ready_file.context("missing required --ready-file")?,
        })
    }
}

async fn run_worker(config: WorkerCli) -> Result<()> {
    let workspace = config
        .workspace
        .canonicalize()
        .unwrap_or_else(|_| config.workspace.clone());
    let daemon_port = find_free_loopback_port().await?;
    let daemon_ws_url = format!("ws://127.0.0.1:{daemon_port}");
    let listen_addr = socket_addr_from_ws_url(&config.listen_ws_url)?;
    let listener = TcpListener::bind(listen_addr)
        .await
        .with_context(|| format!("failed to bind worker listener on {}", config.listen_ws_url))?;
    let local_addr = listener
        .local_addr()
        .context("failed to read worker listener addr")?;
    let worker_ws_url = format!("ws://127.0.0.1:{}", local_addr.port());

    let mut daemon = Command::new("codex")
        .args(["app-server", "--listen", &daemon_ws_url])
        .current_dir(&workspace)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .context("failed to spawn worker-owned codex app-server")?;

    if let Some(stderr) = daemon.stderr.take() {
        let mut stderr_lines = BufReader::new(stderr).lines();
        tokio::spawn(async move {
            while let Ok(Some(line)) = stderr_lines.next_line().await {
                debug!(event = "app_server_ws_worker.codex.stderr", line = %line);
            }
        });
    }

    wait_for_daemon(&daemon_ws_url).await?;
    write_ready_file(
        &config.ready_file,
        &WorkerReadyState {
            worker_ws_url,
            daemon_ws_url: daemon_ws_url.clone(),
        },
    )
    .await?;

    loop {
        tokio::select! {
            result = daemon.wait() => {
                let status = result.context("failed waiting for worker-owned codex app-server")?;
                bail!("worker-owned codex app-server exited unexpectedly: {status:?}");
            }
            accept = listener.accept() => {
                let (stream, _) = accept.context("worker listener accept failed")?;
                let upstream_url = daemon_ws_url.clone();
                tokio::spawn(async move {
                    if let Err(error) = proxy_client_session(stream, &upstream_url).await {
                        warn!(event = "app_server_ws_worker.proxy.failed", error = %error);
                    }
                });
            }
        }
    }
}

async fn proxy_client_session(stream: TcpStream, upstream_url: &str) -> Result<()> {
    let client_ws = accept_async(stream)
        .await
        .context("failed to accept worker websocket client")?;
    let (upstream_ws, _) = connect_async(upstream_url)
        .await
        .with_context(|| format!("failed to connect worker upstream to {upstream_url}"))?;

    let (mut client_sink, mut client_stream) = client_ws.split();
    let (mut upstream_sink, mut upstream_stream) = upstream_ws.split();

    let to_upstream = async {
        while let Some(message) = client_stream.next().await {
            let message = message.context("failed to read worker client websocket message")?;
            upstream_sink
                .send(message)
                .await
                .context("failed to forward worker client message upstream")?;
        }
        Ok::<(), anyhow::Error>(())
    };
    let to_client = async {
        while let Some(message) = upstream_stream.next().await {
            let message = message.context("failed to read worker upstream websocket message")?;
            client_sink
                .send(message)
                .await
                .context("failed to forward worker upstream message to client")?;
        }
        Ok::<(), anyhow::Error>(())
    };

    tokio::select! {
        result = to_upstream => result?,
        result = to_client => result?,
    }

    let _ = upstream_sink.send(WsMessage::Close(None)).await;
    let _ = client_sink.send(WsMessage::Close(None)).await;
    Ok(())
}

async fn write_ready_file(path: &Path, state: &WorkerReadyState) -> Result<()> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    tokio::fs::write(path, format!("{}\n", serde_json::to_string_pretty(state)?))
        .await
        .with_context(|| format!("failed to write {}", path.display()))
}

async fn wait_for_daemon(daemon_ws_url: &str) -> Result<()> {
    for _ in 0..20 {
        if connect_async(daemon_ws_url).await.is_ok() {
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    bail!("worker-owned codex app-server did not become healthy at {daemon_ws_url}");
}

async fn find_free_loopback_port() -> Result<u16> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .context("failed to allocate loopback worker port")?;
    let port = listener
        .local_addr()
        .context("missing worker loopback local addr")?
        .port();
    drop(listener);
    Ok(port)
}

fn socket_addr_from_ws_url(url: &str) -> Result<String> {
    let parsed = url::Url::parse(url).with_context(|| format!("invalid websocket url: {url}"))?;
    if parsed.scheme() != "ws" {
        bail!("worker websocket url must start with ws://");
    }
    if parsed.path() != "/" && !parsed.path().is_empty() {
        bail!("worker websocket url must use root path");
    }
    let host = parsed
        .host_str()
        .context("worker websocket url is missing host")?;
    let port = parsed
        .port()
        .context("worker websocket url is missing port")?;
    Ok(format!("{host}:{port}"))
}
