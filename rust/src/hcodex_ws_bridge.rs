use anyhow::{Context, Result};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::AbortHandle;
use tokio::time::{Duration, timeout};
use tokio_tungstenite::accept_async;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Error as WsError;
use tokio_tungstenite::tungstenite::Message as WsMessage;
use tokio_tungstenite::tungstenite::error::ProtocolError;
use tracing::warn;
use url::Url;

pub(crate) struct PreparedCodexRemote {
    pub(crate) codex_remote_ws_url: String,
    pub(crate) bridge_abort_handle: Option<AbortHandle>,
}

pub(crate) async fn prepare_codex_remote_ws_url(
    launch_ws_url: &str,
) -> Result<PreparedCodexRemote> {
    if is_codex_safe_remote_ws_url(launch_ws_url) {
        return Ok(PreparedCodexRemote {
            codex_remote_ws_url: launch_ws_url.to_owned(),
            bridge_abort_handle: None,
        });
    }

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .context("failed to bind local hcodex websocket bridge")?;
    let local_addr = listener
        .local_addr()
        .context("failed to determine local hcodex websocket bridge addr")?;
    let local_bridge_ws_url = format!("ws://127.0.0.1:{}", local_addr.port());
    let launch_ws_url = launch_ws_url.to_owned();
    let bridge_task = tokio::spawn(async move {
        if let Err(error) = run_bridge(listener, &launch_ws_url).await {
            warn!(
                event = "hcodex_ws_bridge.failed",
                upstream_launch_ws_url = %launch_ws_url,
                error = %error,
                "hcodex websocket bridge failed"
            );
        }
    });

    Ok(PreparedCodexRemote {
        codex_remote_ws_url: local_bridge_ws_url,
        bridge_abort_handle: Some(bridge_task.abort_handle()),
    })
}

fn is_codex_safe_remote_ws_url(remote_ws_url: &str) -> bool {
    let Ok(parsed) = Url::parse(remote_ws_url) else {
        return false;
    };
    matches!(parsed.scheme(), "ws" | "wss")
        && parsed.host_str().is_some()
        && parsed.port().is_some()
        && parsed.path() == "/"
        && parsed.query().is_none()
        && parsed.fragment().is_none()
}

async fn run_bridge(listener: TcpListener, upstream_launch_ws_url: &str) -> Result<()> {
    let mut accepted_any_client = false;
    loop {
        let accept_result = if accepted_any_client {
            match timeout(Duration::from_secs(2), listener.accept()).await {
                Ok(result) => result.context("failed to accept local hcodex websocket client")?,
                Err(_) => break,
            }
        } else {
            listener
                .accept()
                .await
                .context("failed to accept local hcodex websocket client")?
        };
        accepted_any_client = true;
        bridge_single_client(accept_result.0, upstream_launch_ws_url).await?;
    }

    Ok(())
}

async fn bridge_single_client(stream: TcpStream, upstream_launch_ws_url: &str) -> Result<()> {
    let client_ws = accept_async(stream)
        .await
        .context("failed to accept local hcodex websocket handshake")?;
    let (upstream_ws, _) = connect_async(upstream_launch_ws_url)
        .await
        .with_context(|| {
            format!("failed to connect bridge to upstream websocket {upstream_launch_ws_url}")
        })?;
    let (mut client_write, mut client_read) = futures_util::StreamExt::split(client_ws);
    let (mut upstream_write, mut upstream_read) = futures_util::StreamExt::split(upstream_ws);

    let client_to_upstream = async {
        while let Some(client_message) = futures_util::StreamExt::next(&mut client_read).await {
            let client_message = match client_message {
                Ok(client_message) => client_message,
                Err(error) if is_graceful_disconnect(&error) => break,
                Err(error) => {
                    return Err(error).context("failed to read local hcodex websocket message");
                }
            };
            let is_close = matches!(client_message, WsMessage::Close(_));
            match futures_util::SinkExt::send(&mut upstream_write, client_message).await {
                Ok(()) => {}
                Err(error) if is_close && is_graceful_disconnect(&error) => break,
                Err(error) => {
                    return Err(error)
                        .context("failed to forward local hcodex websocket message upstream");
                }
            }
            if is_close {
                break;
            }
        }
        let _ = futures_util::SinkExt::close(&mut upstream_write).await;
        Result::<()>::Ok(())
    };

    let upstream_to_client = async {
        while let Some(upstream_message) = futures_util::StreamExt::next(&mut upstream_read).await {
            let upstream_message = match upstream_message {
                Ok(upstream_message) => upstream_message,
                Err(error) if is_graceful_disconnect(&error) => break,
                Err(error) => {
                    return Err(error).context("failed to read upstream websocket message");
                }
            };
            let is_close = matches!(upstream_message, WsMessage::Close(_));
            match futures_util::SinkExt::send(&mut client_write, upstream_message).await {
                Ok(()) => {}
                Err(error) if is_close && is_graceful_disconnect(&error) => break,
                Err(error) => {
                    return Err(error).context(
                        "failed to forward upstream websocket message to local hcodex client",
                    );
                }
            }
            if is_close {
                break;
            }
        }
        let _ = futures_util::SinkExt::close(&mut client_write).await;
        Result::<()>::Ok(())
    };

    tokio::try_join!(client_to_upstream, upstream_to_client)?;

    Ok(())
}

fn is_graceful_disconnect(error: &WsError) -> bool {
    matches!(
        error,
        WsError::ConnectionClosed
            | WsError::Protocol(ProtocolError::ResetWithoutClosingHandshake)
            | WsError::Protocol(ProtocolError::SendAfterClosing)
    )
}

#[cfg(test)]
mod tests {
    use super::{prepare_codex_remote_ws_url, run_bridge};
    use anyhow::{Context, Result};
    use std::sync::{Arc, Mutex as StdMutex};
    use tokio::net::TcpListener;
    use tokio::time::{Duration, timeout};
    use tokio_tungstenite::accept_hdr_async;
    use tokio_tungstenite::connect_async;
    use tokio_tungstenite::tungstenite::Message as WsMessage;
    use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};

    #[tokio::test]
    async fn bare_remote_ws_url_passes_through_without_bridge() -> Result<()> {
        let prepared = prepare_codex_remote_ws_url("ws://127.0.0.1:4500").await?;
        assert_eq!(prepared.codex_remote_ws_url, "ws://127.0.0.1:4500");
        assert!(prepared.bridge_abort_handle.is_none());
        Ok(())
    }

    #[tokio::test]
    async fn launch_url_with_query_is_bridged_to_local_canonical_ws_url() -> Result<()> {
        let upstream_listener = TcpListener::bind("127.0.0.1:0").await?;
        let upstream_addr = upstream_listener.local_addr()?;
        let upstream_launch_ws_url = format!(
            "ws://127.0.0.1:{}/?launch_ticket=test-ticket",
            upstream_addr.port()
        );
        let captured_query = Arc::new(StdMutex::new(None::<Option<String>>));
        let captured_query_for_task = captured_query.clone();
        let upstream_task = tokio::spawn(async move {
            let (stream, _) = upstream_listener.accept().await?;
            let mut ws = accept_hdr_async(stream, move |request: &Request, response: Response| {
                *captured_query_for_task.lock().expect("capture query") =
                    Some(request.uri().query().map(str::to_owned));
                Ok(response)
            })
            .await?;
            futures_util::SinkExt::send(&mut ws, WsMessage::Close(None)).await?;
            Result::<()>::Ok(())
        });

        let prepared = prepare_codex_remote_ws_url(&upstream_launch_ws_url).await?;
        assert_ne!(prepared.codex_remote_ws_url, upstream_launch_ws_url);
        assert!(
            prepared.codex_remote_ws_url.starts_with("ws://127.0.0.1:"),
            "bridge should expose a local canonical ws://host:port URL"
        );
        assert!(prepared.bridge_abort_handle.is_some());

        let (mut client_ws, _) = connect_async(&prepared.codex_remote_ws_url).await?;
        let _ = futures_util::StreamExt::next(&mut client_ws).await;
        drop(client_ws);

        let _ = timeout(Duration::from_secs(2), upstream_task).await??;
        assert_eq!(
            *captured_query.lock().expect("read captured query"),
            Some(Some("launch_ticket=test-ticket".to_owned()))
        );
        if let Some(abort_handle) = prepared.bridge_abort_handle.as_ref() {
            abort_handle.abort();
        }
        Ok(())
    }

    #[tokio::test]
    async fn bridge_reuses_full_launch_url_for_upstream_handshake() -> Result<()> {
        let upstream_listener = TcpListener::bind("127.0.0.1:0").await?;
        let upstream_addr = upstream_listener.local_addr()?;
        let upstream_launch_ws_url = format!(
            "ws://127.0.0.1:{}/bridge/path?launch_ticket=sideband",
            upstream_addr.port()
        );
        let captured_path = Arc::new(StdMutex::new(None::<(String, Option<String>)>));
        let captured_path_for_task = captured_path.clone();
        let upstream_task = tokio::spawn(async move {
            let (stream, _) = upstream_listener.accept().await?;
            let mut ws = accept_hdr_async(stream, move |request: &Request, response: Response| {
                *captured_path_for_task.lock().expect("capture path") = Some((
                    request.uri().path().to_owned(),
                    request.uri().query().map(str::to_owned),
                ));
                Ok(response)
            })
            .await?;
            let message = futures_util::StreamExt::next(&mut ws)
                .await
                .context("missing upstream websocket message")??;
            futures_util::SinkExt::send(&mut ws, message).await?;
            Result::<()>::Ok(())
        });

        let bridge_listener = TcpListener::bind("127.0.0.1:0").await?;
        let bridge_addr = bridge_listener.local_addr()?;
        let bridge_task = tokio::spawn({
            let upstream_launch_ws_url = upstream_launch_ws_url.clone();
            async move { run_bridge(bridge_listener, &upstream_launch_ws_url).await }
        });

        let local_bridge_ws_url = format!("ws://127.0.0.1:{}", bridge_addr.port());
        let (mut client_ws, _) = connect_async(&local_bridge_ws_url).await?;
        futures_util::SinkExt::send(&mut client_ws, WsMessage::Text("ping".into())).await?;
        let echoed = timeout(
            Duration::from_secs(2),
            futures_util::StreamExt::next(&mut client_ws),
        )
        .await?
        .context("missing echoed websocket message")??;
        assert_eq!(echoed.into_text()?, "ping");
        drop(client_ws);

        let _ = timeout(Duration::from_secs(2), upstream_task).await??;
        let captured = captured_path.lock().expect("read captured path").clone();
        assert_eq!(
            captured,
            Some((
                "/bridge/path".to_owned(),
                Some("launch_ticket=sideband".to_owned())
            ))
        );
        bridge_task.abort();
        Ok(())
    }
}
