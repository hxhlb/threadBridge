use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;
use serde_json::Value;
use teloxide::Bot;
use teloxide::payloads::SendMessageSetters;
use teloxide::requests::Requester;
use teloxide::types::{MessageId, ThreadId};
use tokio::sync::Mutex;
use tokio::task::AbortHandle;
use tracing::warn;

use crate::codex::{
    CodexServerNotification, CodexServerRequest, CodexThreadEvent, observe_thread_with_handlers,
};
use crate::collaboration_mode::CollaborationMode;
use crate::interactive::{
    InteractiveRequestRegistry, ServerRequestResolvedNotification, ToolRequestUserInputParams,
};
use crate::process_transcript::process_entry_from_codex_event;
use crate::repository::{ThreadRepository, TranscriptMirrorOrigin, TranscriptMirrorPhase};
use crate::telegram_runtime::{
    final_reply::compose_visible_final_reply, render_request_user_input_prompt,
    request_user_input_markup, send_plan_implementation_prompt,
};
use crate::workspace_status::{
    record_hcodex_ingress_completed, record_hcodex_ingress_preview_text,
    record_hcodex_ingress_process_event, record_hcodex_ingress_prompt,
};

#[derive(Debug, Clone)]
pub(crate) struct TelegramInteractiveBridge {
    pub(crate) bot: Bot,
    pub(crate) registry: InteractiveRequestRegistry,
}

#[derive(Debug, Clone)]
pub struct AppServerMirrorObserverManager {
    repository: ThreadRepository,
    turn_modes: Arc<Mutex<HashMap<String, CollaborationMode>>>,
    inner: Arc<Mutex<HashMap<String, RunningObserver>>>,
    telegram_bridge: Arc<Mutex<Option<TelegramInteractiveBridge>>>,
}

#[derive(Debug, Clone)]
struct RunningObserver {
    thread_id: String,
    abort_handle: AbortHandle,
}

#[derive(Debug, Default)]
struct ObserverState {
    latest_assistant_message: String,
    latest_completed_plan_text: Option<String>,
}

impl AppServerMirrorObserverManager {
    pub(crate) fn new(
        repository: ThreadRepository,
        turn_modes: Arc<Mutex<HashMap<String, CollaborationMode>>>,
        telegram_bridge: Arc<Mutex<Option<TelegramInteractiveBridge>>>,
    ) -> Self {
        Self {
            repository,
            turn_modes,
            inner: Arc::new(Mutex::new(HashMap::new())),
            telegram_bridge,
        }
    }

    pub async fn ensure_thread_observer(
        &self,
        workspace_path: &Path,
        daemon_ws_url: &str,
        thread_key: &str,
        thread_id: &str,
    ) -> Result<()> {
        let key = observer_key(workspace_path, thread_key);
        let mut inner = self.inner.lock().await;
        if let Some(existing) = inner.get(&key) {
            if existing.thread_id == thread_id {
                return Ok(());
            }
            existing.abort_handle.abort();
        }

        let repository = self.repository.clone();
        let workspace_path = workspace_path
            .canonicalize()
            .unwrap_or_else(|_| workspace_path.to_path_buf());
        let daemon_ws_url = daemon_ws_url.to_owned();
        let thread_key = thread_key.to_owned();
        let thread_id = thread_id.to_owned();
        let observer_thread_id = thread_id.clone();
        let turn_modes = self.turn_modes.clone();
        let telegram_bridge = self.telegram_bridge.clone();
        let task = tokio::spawn(async move {
            if let Err(error) = run_thread_observer(
                repository,
                turn_modes,
                telegram_bridge,
                workspace_path,
                daemon_ws_url,
                thread_key,
                observer_thread_id,
            )
            .await
            {
                warn!(event = "app_server_observer.failed", error = %error);
            }
        });
        inner.insert(
            key,
            RunningObserver {
                thread_id,
                abort_handle: task.abort_handle(),
            },
        );
        Ok(())
    }

    pub async fn stop_thread_observer(&self, workspace_path: &Path, thread_key: &str) {
        let key = observer_key(workspace_path, thread_key);
        if let Some(existing) = self.inner.lock().await.remove(&key) {
            existing.abort_handle.abort();
        }
    }

    pub async fn record_turn_mode(&self, turn_id: &str, mode: CollaborationMode) {
        self.turn_modes
            .lock()
            .await
            .insert(turn_id.to_owned(), mode);
    }
}

fn observer_key(workspace_path: &Path, thread_key: &str) -> String {
    format!(
        "{}::{thread_key}",
        workspace_path
            .canonicalize()
            .unwrap_or_else(|_| workspace_path.to_path_buf())
            .display()
    )
}

async fn run_thread_observer(
    repository: ThreadRepository,
    turn_modes: Arc<Mutex<HashMap<String, CollaborationMode>>>,
    telegram_bridge: Arc<Mutex<Option<TelegramInteractiveBridge>>>,
    workspace_path: PathBuf,
    daemon_ws_url: String,
    thread_key: String,
    thread_id: String,
) -> Result<()> {
    let state = Arc::new(Mutex::new(ObserverState::default()));
    observe_thread_with_handlers(
        &daemon_ws_url,
        &thread_id,
        {
            let repository = repository.clone();
            let workspace_path = workspace_path.clone();
            let thread_key = thread_key.clone();
            let observer_thread_id = thread_id.clone();
            let turn_modes = turn_modes.clone();
            let telegram_bridge = telegram_bridge.clone();
            let state = state.clone();
            move |event| {
                let repository = repository.clone();
                let workspace_path = workspace_path.clone();
                let thread_key = thread_key.clone();
                let observer_thread_id = observer_thread_id.clone();
                let turn_modes = turn_modes.clone();
                let telegram_bridge = telegram_bridge.clone();
                let state = state.clone();
                async move {
                    handle_observer_event(
                        &repository,
                        &workspace_path,
                        &thread_key,
                        &observer_thread_id,
                        &turn_modes,
                        &telegram_bridge,
                        &state,
                        event,
                    )
                    .await
                }
            }
        },
        {
            let repository = repository.clone();
            let thread_key = thread_key.clone();
            let telegram_bridge = telegram_bridge.clone();
            move |request| {
                let repository = repository.clone();
                let thread_key = thread_key.clone();
                let telegram_bridge = telegram_bridge.clone();
                async move {
                    handle_server_request(&repository, &thread_key, &telegram_bridge, request).await
                }
            }
        },
        {
            let telegram_bridge = telegram_bridge.clone();
            move |notification| {
                let telegram_bridge = telegram_bridge.clone();
                async move { handle_server_notification(&telegram_bridge, notification).await }
            }
        },
    )
    .await
}

async fn handle_observer_event(
    repository: &ThreadRepository,
    workspace_path: &Path,
    thread_key: &str,
    thread_id: &str,
    turn_modes: &Arc<Mutex<HashMap<String, CollaborationMode>>>,
    telegram_bridge: &Arc<Mutex<Option<TelegramInteractiveBridge>>>,
    state: &Arc<Mutex<ObserverState>>,
    event: CodexThreadEvent,
) -> Result<()> {
    if let Some(prompt) = extract_user_prompt_text(&event) {
        record_hcodex_ingress_prompt(workspace_path, thread_id, &prompt).await?;
    }

    if let Some(text) = extract_agent_message_text(&event) {
        state.lock().await.latest_assistant_message = text.clone();
        record_hcodex_ingress_preview_text(workspace_path, thread_id, &text).await?;
    }

    if let Some(plan_text) = extract_completed_plan_text(&event) {
        state.lock().await.latest_completed_plan_text = Some(plan_text);
    }

    if let Some(entry) =
        process_entry_from_codex_event(&event, thread_id, TranscriptMirrorOrigin::Tui)
    {
        let phase = match entry.phase {
            Some(TranscriptMirrorPhase::Plan) => Some("plan"),
            Some(TranscriptMirrorPhase::Tool) => Some("tool"),
            None => None,
        };
        if let Some(phase) = phase {
            record_hcodex_ingress_process_event(workspace_path, thread_id, phase, &entry.text)
                .await?;
        }
    }

    match event {
        CodexThreadEvent::TurnCompleted { turn_id, .. } => {
            finalize_turn(
                repository,
                workspace_path,
                thread_key,
                thread_id,
                turn_id.as_deref(),
                turn_modes,
                telegram_bridge,
                state,
                None,
            )
            .await?;
        }
        CodexThreadEvent::TurnFailed { turn_id, error } => {
            finalize_turn(
                repository,
                workspace_path,
                thread_key,
                thread_id,
                turn_id.as_deref(),
                turn_modes,
                telegram_bridge,
                state,
                Some(error.to_string()),
            )
            .await?;
        }
        CodexThreadEvent::ThreadStarted { .. }
        | CodexThreadEvent::TurnStarted
        | CodexThreadEvent::Error { .. }
        | CodexThreadEvent::ItemStarted { .. }
        | CodexThreadEvent::ItemUpdated { .. }
        | CodexThreadEvent::ItemCompleted { .. } => {}
    }
    Ok(())
}

async fn finalize_turn(
    repository: &ThreadRepository,
    workspace_path: &Path,
    thread_key: &str,
    thread_id: &str,
    turn_id: Option<&str>,
    turn_modes: &Arc<Mutex<HashMap<String, CollaborationMode>>>,
    telegram_bridge: &Arc<Mutex<Option<TelegramInteractiveBridge>>>,
    state: &Arc<Mutex<ObserverState>>,
    fallback_error: Option<String>,
) -> Result<()> {
    let mut state_guard = state.lock().await;
    let final_text = compose_visible_final_reply(
        &state_guard.latest_assistant_message,
        state_guard.latest_completed_plan_text.as_deref(),
    )
    .or_else(|| fallback_error.as_deref().map(str::to_owned));
    record_hcodex_ingress_completed(workspace_path, thread_id, turn_id, final_text.as_deref())
        .await?;
    let plan_mode = match turn_id {
        Some(turn_id) => turn_modes.lock().await.remove(turn_id),
        None => None,
    };
    if plan_mode == Some(CollaborationMode::Plan)
        && state_guard.latest_completed_plan_text.is_some()
    {
        maybe_send_plan_prompt_from_bridge(repository, thread_key, telegram_bridge).await?;
    }
    state_guard.latest_assistant_message.clear();
    state_guard.latest_completed_plan_text = None;
    Ok(())
}

async fn handle_server_request(
    repository: &ThreadRepository,
    thread_key: &str,
    telegram_bridge: &Arc<Mutex<Option<TelegramInteractiveBridge>>>,
    request: CodexServerRequest,
) -> Result<()> {
    let CodexServerRequest::RequestUserInput { request_id, params } = request;
    maybe_bridge_request_user_input(repository, thread_key, telegram_bridge, request_id, params)
        .await
}

async fn handle_server_notification(
    telegram_bridge: &Arc<Mutex<Option<TelegramInteractiveBridge>>>,
    notification: CodexServerNotification,
) -> Result<()> {
    let CodexServerNotification::ServerRequestResolved(resolved) = notification;
    maybe_bridge_resolved_request(telegram_bridge, resolved).await
}

async fn maybe_bridge_request_user_input(
    repository: &ThreadRepository,
    thread_key: &str,
    telegram_bridge: &Arc<Mutex<Option<TelegramInteractiveBridge>>>,
    request_id: i64,
    params: ToolRequestUserInputParams,
) -> Result<()> {
    let Some(bridge) = telegram_bridge.lock().await.clone() else {
        return Ok(());
    };
    let Some(record) = repository.find_active_thread_by_key(thread_key).await? else {
        return Ok(());
    };
    let Some(telegram_thread_id) = record.metadata.message_thread_id else {
        return Ok(());
    };
    if params.questions.iter().any(|question| question.is_secret) {
        return Ok(());
    }
    let snapshot = bridge
        .registry
        .register_tui(
            record.metadata.chat_id,
            telegram_thread_id,
            thread_key.to_owned(),
            request_id,
            params,
        )
        .await?;
    let text = render_request_user_input_prompt(&snapshot);
    let request = bridge
        .bot
        .send_message(teloxide::types::ChatId(record.metadata.chat_id), text)
        .message_thread_id(ThreadId(MessageId(telegram_thread_id)));
    let sent =
        if let Some(markup) = request_user_input_markup(snapshot.request_id, &snapshot.question) {
            request.reply_markup(markup).await?
        } else {
            request.await?
        };
    bridge
        .registry
        .set_prompt_message_id(record.metadata.chat_id, telegram_thread_id, sent.id.0)
        .await;
    Ok(())
}

async fn maybe_bridge_resolved_request(
    telegram_bridge: &Arc<Mutex<Option<TelegramInteractiveBridge>>>,
    resolved: ServerRequestResolvedNotification,
) -> Result<()> {
    let Some(bridge) = telegram_bridge.lock().await.clone() else {
        return Ok(());
    };
    let Some(resolved_request) = bridge
        .registry
        .resolve_request_id(&resolved.thread_id, &resolved.request_id)
        .await
    else {
        return Ok(());
    };
    if let Some(message_id) = resolved_request.prompt_message_id {
        let _ = bridge
            .bot
            .edit_message_text(
                teloxide::types::ChatId(resolved_request.chat_id),
                MessageId(message_id),
                "Info: Questions resolved.",
            )
            .await;
    }
    Ok(())
}

async fn maybe_send_plan_prompt_from_bridge(
    repository: &ThreadRepository,
    thread_key: &str,
    telegram_bridge: &Arc<Mutex<Option<TelegramInteractiveBridge>>>,
) -> Result<()> {
    let Some(bridge) = telegram_bridge.lock().await.clone() else {
        return Ok(());
    };
    let Some(record) = repository.find_active_thread_by_key(thread_key).await? else {
        return Ok(());
    };
    let Some(telegram_thread_id) = record.metadata.message_thread_id else {
        return Ok(());
    };
    send_plan_implementation_prompt(
        &bridge.bot,
        teloxide::types::ChatId(record.metadata.chat_id),
        ThreadId(MessageId(telegram_thread_id)),
    )
    .await?;
    Ok(())
}

fn extract_agent_message_text(event: &CodexThreadEvent) -> Option<String> {
    let item = match event {
        CodexThreadEvent::ItemUpdated { item } | CodexThreadEvent::ItemCompleted { item } => item,
        _ => return None,
    };
    if item.get("type").and_then(Value::as_str) != Some("agent_message") {
        return None;
    }
    item.get("text")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

fn extract_completed_plan_text(event: &CodexThreadEvent) -> Option<String> {
    let item = match event {
        CodexThreadEvent::ItemCompleted { item } => item,
        _ => return None,
    };
    if item.get("type").and_then(Value::as_str) != Some("plan") {
        return None;
    }
    item.get("text")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

fn extract_user_prompt_text(event: &CodexThreadEvent) -> Option<String> {
    let item = match event {
        CodexThreadEvent::ItemCompleted { item } => item,
        _ => return None,
    };
    if item.get("type").and_then(Value::as_str) != Some("user_message") {
        return None;
    }
    item.get("text")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
        .or_else(|| {
            item.get("content")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|value| value.get("text").and_then(Value::as_str))
                        .map(str::trim)
                        .filter(|text| !text.is_empty())
                        .collect::<Vec<_>>()
                        .join("\n\n")
                })
                .filter(|text| !text.is_empty())
        })
}
