use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Result;
use teloxide::prelude::*;
use teloxide::types::{MessageId, ThreadId};
use tracing::{info, warn};

use super::*;
use crate::repository::{
    SessionAttachmentState, ThreadStatus, TranscriptMirrorDelivery, TranscriptMirrorEntry,
    TranscriptMirrorOrigin, TranscriptMirrorRole,
};
use crate::workspace_status::{
    CliOwnerClaim, SessionCurrentStatus, SessionStatusOwner, WorkspaceAggregateStatus,
    WorkspaceStatusEventRecord, events_path, read_cli_owner_claim, read_session_status,
    record_bot_status_event,
};

const TELEGRAM_TOPIC_TITLE_MAX_CHARS: usize = 128;
const STARTUP_STALE_BUSY_RECOVERED_LOG: &str =
    "Recovered stale busy state from previous threadBridge process during startup.";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StaleBusyReconciliationReport {
    pub scanned_threads: usize,
    pub unique_sessions: usize,
    pub recovered_sessions: usize,
    pub recovered_threads: usize,
    pub skipped_threads: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CliTopicMarker {
    None,
    Cli,
    CliConflict,
    Attach,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CliViewerInjectionState {
    lifecycle_id: String,
    shell_pid: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CliViewerInjectionTransition {
    None,
    Enter(CliViewerInjectionState),
    Exit,
    ExitAndEnter(CliViewerInjectionState),
    ClearSilently,
}

fn thread_id_from_i32(value: i32) -> ThreadId {
    ThreadId(MessageId(value))
}

fn workspace_basename(workspace_path: Option<&Path>) -> Option<String> {
    workspace_path
        .and_then(|path| path.file_name())
        .and_then(|name| name.to_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn truncate_topic_base(base: &str, suffix: &str) -> String {
    let suffix_len = suffix.chars().count();
    if suffix_len >= TELEGRAM_TOPIC_TITLE_MAX_CHARS {
        return suffix
            .chars()
            .take(TELEGRAM_TOPIC_TITLE_MAX_CHARS)
            .collect::<String>();
    }
    let max_base_len = TELEGRAM_TOPIC_TITLE_MAX_CHARS - suffix_len;
    let base_len = base.chars().count();
    if base_len <= max_base_len {
        return format!("{base}{suffix}");
    }
    let ellipsis = "...";
    let keep_len = max_base_len.saturating_sub(ellipsis.chars().count());
    let mut truncated = base.chars().take(keep_len).collect::<String>();
    truncated.push_str(ellipsis);
    format!("{truncated}{suffix}")
}

fn workspace_cli_conflict(
    aggregate: Option<&WorkspaceAggregateStatus>,
    owner_claim: Option<&CliOwnerClaim>,
) -> bool {
    let Some(aggregate) = aggregate else {
        return false;
    };
    if aggregate.live_cli_session_ids.is_empty() {
        return false;
    }
    let Some(owner_claim) = owner_claim else {
        return true;
    };
    if aggregate.live_cli_session_ids.len() > 1 {
        return true;
    }
    let Some(expected_session_id) = owner_claim.session_id.as_deref() else {
        return false;
    };
    aggregate
        .live_cli_session_ids
        .iter()
        .all(|item| item != expected_session_id)
}

pub(crate) fn cli_topic_marker_for_record(
    record: &ThreadRecord,
    session: Option<&SessionBinding>,
    aggregate: Option<&WorkspaceAggregateStatus>,
    owner_claim: Option<&CliOwnerClaim>,
) -> CliTopicMarker {
    if session.is_some_and(|binding| binding.attachment_state == SessionAttachmentState::CliHandoff)
    {
        return CliTopicMarker::Attach;
    }
    if workspace_cli_conflict(aggregate, owner_claim) {
        return CliTopicMarker::CliConflict;
    }
    if owner_claim.is_some_and(|claim| claim.thread_key == record.metadata.thread_key) {
        return CliTopicMarker::Cli;
    }
    CliTopicMarker::None
}

pub(crate) fn render_topic_title(
    record: &ThreadRecord,
    workspace_path: Option<&Path>,
    marker: CliTopicMarker,
) -> String {
    let base = record
        .metadata
        .title
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .or_else(|| workspace_basename(workspace_path))
        .unwrap_or_else(|| "Unbound".to_owned());

    let mut suffix = String::new();
    match marker {
        CliTopicMarker::Attach => suffix.push_str(" · attach"),
        CliTopicMarker::Cli => suffix.push_str(" · cli"),
        CliTopicMarker::CliConflict => suffix.push_str(" · cli!"),
        CliTopicMarker::None => {}
    }
    if record.metadata.session_broken {
        suffix.push_str(" · broken");
    }

    truncate_topic_base(&base, &suffix)
}

pub(crate) fn cli_marker_label(marker: CliTopicMarker) -> &'static str {
    match marker {
        CliTopicMarker::None => "none",
        CliTopicMarker::Cli => ".cli",
        CliTopicMarker::CliConflict => ".cli!",
        CliTopicMarker::Attach => ".attach",
    }
}

fn cli_viewer_injection_state(
    marker: CliTopicMarker,
    record: &ThreadRecord,
    owner_claim: Option<&CliOwnerClaim>,
) -> Option<CliViewerInjectionState> {
    if marker != CliTopicMarker::Cli {
        return None;
    }
    let owner_claim = owner_claim?;
    if owner_claim.thread_key != record.metadata.thread_key {
        return None;
    }
    Some(CliViewerInjectionState {
        lifecycle_id: format!(
            "{}:{}:{}",
            owner_claim.thread_key, owner_claim.shell_pid, owner_claim.started_at
        ),
        shell_pid: owner_claim.shell_pid,
    })
}

fn cli_viewer_injection_transition(
    previous: Option<&CliViewerInjectionState>,
    marker: CliTopicMarker,
    record: &ThreadRecord,
    owner_claim: Option<&CliOwnerClaim>,
) -> CliViewerInjectionTransition {
    let current = cli_viewer_injection_state(marker, record, owner_claim);
    match marker {
        CliTopicMarker::Attach | CliTopicMarker::CliConflict => {
            if previous.is_some() {
                CliViewerInjectionTransition::ClearSilently
            } else {
                CliViewerInjectionTransition::None
            }
        }
        CliTopicMarker::Cli => match (previous, current) {
            (None, Some(current)) => CliViewerInjectionTransition::Enter(current),
            (Some(previous), Some(current)) if previous.lifecycle_id == current.lifecycle_id => {
                CliViewerInjectionTransition::None
            }
            (Some(_), Some(current)) => CliViewerInjectionTransition::ExitAndEnter(current),
            _ => CliViewerInjectionTransition::None,
        },
        CliTopicMarker::None => {
            if previous.is_some() {
                CliViewerInjectionTransition::Exit
            } else {
                CliViewerInjectionTransition::None
            }
        }
    }
}

fn cli_viewer_enter_message(shell_pid: u32) -> String {
    format!("as shell {shell_pid} viewer")
}

fn cli_viewer_exit_message() -> &'static str {
    "exit session viewer"
}

pub(crate) async fn refresh_thread_topic_title(
    bot: &Bot,
    state: &AppState,
    record: &ThreadRecord,
    source: &'static str,
) -> Result<()> {
    let Some(message_thread_id) = record.metadata.message_thread_id else {
        return Ok(());
    };
    let session = state.repository.read_session_binding(record).await?;
    let workspace_path = session
        .as_ref()
        .and_then(|binding| binding.workspace_cwd.as_deref())
        .map(PathBuf::from);
    let aggregate = if let Some(path) = workspace_path.as_ref() {
        Some(read_workspace_status_with_cache(&state.workspace_status_cache, path).await?)
    } else {
        None
    };
    let owner_claim = if let Some(path) = workspace_path.as_ref() {
        read_cli_owner_claim(path).await?
    } else {
        None
    };
    let title = render_topic_title(
        record,
        workspace_path.as_deref(),
        cli_topic_marker_for_record(
            record,
            session.as_ref(),
            aggregate.as_ref(),
            owner_claim.as_ref(),
        ),
    );
    apply_thread_topic_title(
        bot,
        record,
        workspace_path.as_deref(),
        message_thread_id,
        &title,
        source,
    )
    .await
}

async fn apply_thread_topic_title(
    bot: &Bot,
    record: &ThreadRecord,
    workspace_path: Option<&Path>,
    message_thread_id: i32,
    title: &str,
    source: &'static str,
) -> Result<()> {
    match bot
        .edit_forum_topic(
            ChatId(record.metadata.chat_id),
            thread_id_from_i32(message_thread_id),
        )
        .name(title.to_owned())
        .await
    {
        Ok(_) => Ok(()),
        Err(error) => {
            let workspace = workspace_path
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "unbound".to_owned());
            warn!(
                event = "telegram.topic_title.refresh_failed",
                source = source,
                thread_key = %record.metadata.thread_key,
                chat_id = record.metadata.chat_id,
                message_thread_id,
                workspace = %workspace,
                stored_title = record.metadata.title.as_deref().unwrap_or(""),
                desired_title = %title,
                session_broken = record.metadata.session_broken,
                error = %error,
                "failed to update Telegram forum topic title"
            );
            Err(error.into())
        }
    }
}

pub(crate) fn busy_text_message(
    snapshot: &SessionCurrentStatus,
    image_saved: bool,
) -> &'static str {
    match snapshot.owner {
        SessionStatusOwner::Cli if image_saved => {
            "Image saved. Analysis will stay pending until the attached CLI session finishes its current turn."
        }
        SessionStatusOwner::Cli => {
            "The attached CLI session is already running a turn. Wait for it to finish before sending a new Telegram request."
        }
        SessionStatusOwner::Bot => {
            "This thread's selected Codex session is already handling another Telegram request. Wait for it to finish before sending a new one."
        }
    }
}

pub(crate) fn busy_command_message(snapshot: &SessionCurrentStatus) -> &'static str {
    match snapshot.owner {
        SessionStatusOwner::Cli => {
            "The attached CLI session is already running a turn. Wait for it to finish before changing this thread's session selection."
        }
        SessionStatusOwner::Bot => {
            "This thread's selected Codex session is already handling another Telegram request. Wait for it to finish before changing session state."
        }
    }
}

pub(crate) fn cli_owned_text_message(image_saved: bool) -> &'static str {
    if image_saved {
        "Image saved. Local Codex CLI currently owns this session. Run /attach_cli_session to take it over before starting analysis."
    } else {
        "Local Codex CLI currently owns this session. Run /attach_cli_session to take it over in Telegram."
    }
}

pub(crate) fn cli_owned_command_message() -> &'static str {
    "Local Codex CLI currently owns this session. Run /attach_cli_session to take it over before changing thread session state."
}

fn session_reconciliation_key(workspace_path: &Path, session_id: &str) -> String {
    let workspace = workspace_path
        .canonicalize()
        .unwrap_or_else(|_| workspace_path.to_path_buf())
        .display()
        .to_string();
    format!("{workspace}::{session_id}")
}

fn should_recover_stale_bot_busy(snapshot: &SessionCurrentStatus) -> bool {
    snapshot.owner == SessionStatusOwner::Bot && snapshot.phase.is_turn_busy()
}

pub async fn reconcile_stale_bot_busy_sessions(
    state: &AppState,
) -> Result<StaleBusyReconciliationReport> {
    reconcile_stale_bot_busy_sessions_for_repository(&state.repository).await
}

async fn reconcile_stale_bot_busy_sessions_for_repository(
    repository: &ThreadRepository,
) -> Result<StaleBusyReconciliationReport> {
    let records = repository.list_active_threads().await?;
    let mut report = StaleBusyReconciliationReport::default();
    let mut recovery_by_session: HashMap<String, bool> = HashMap::new();

    for record in records {
        report.scanned_threads += 1;
        let Some(binding) = repository.read_session_binding(&record).await? else {
            report.skipped_threads += 1;
            continue;
        };
        let Some(session_id) = usable_bound_session_id(Some(&binding)) else {
            report.skipped_threads += 1;
            continue;
        };
        let Some(workspace_cwd) = binding.workspace_cwd.as_deref() else {
            report.skipped_threads += 1;
            continue;
        };

        let workspace_path = PathBuf::from(workspace_cwd);
        let session_key = session_reconciliation_key(&workspace_path, session_id);
        let recovered = if let Some(known) = recovery_by_session.get(&session_key).copied() {
            known
        } else {
            report.unique_sessions += 1;
            let recovered =
                if let Some(snapshot) = read_session_status(&workspace_path, session_id).await? {
                    if should_recover_stale_bot_busy(&snapshot) {
                        info!(
                            event = "workspace_status.reconcile_stale_bot_busy.recovered",
                            thread_key = %record.metadata.thread_key,
                            workspace = %workspace_path.display(),
                            session_id,
                            previous_phase = ?snapshot.phase,
                            previous_turn_id = snapshot.turn_id.as_deref().unwrap_or(""),
                            "recovered stale bot-owned busy session snapshot"
                        );
                        record_bot_status_event(
                            &workspace_path,
                            "bot_turn_recovered",
                            Some(session_id),
                            snapshot.turn_id.as_deref(),
                            None,
                        )
                        .await?;
                        report.recovered_sessions += 1;
                        true
                    } else {
                        false
                    }
                } else {
                    false
                };
            recovery_by_session.insert(session_key.clone(), recovered);
            recovered
        };

        if recovered {
            repository
                .append_log(
                    &record,
                    LogDirection::System,
                    STARTUP_STALE_BUSY_RECOVERED_LOG,
                    None,
                )
                .await?;
            report.recovered_threads += 1;
        } else {
            report.skipped_threads += 1;
        }
    }

    Ok(report)
}

pub async fn spawn_workspace_status_watcher(bot: Bot, state: AppState) {
    tokio::spawn(async move {
        let mut applied_titles: HashMap<String, String> = HashMap::new();
        let mut viewer_injections: HashMap<String, CliViewerInjectionState> = HashMap::new();
        let mut workspace_event_offsets: HashMap<String, usize> = HashMap::new();
        let mut pending_cli_user_prompts: HashSet<String> = HashSet::new();
        loop {
            if let Err(error) = sync_workspace_titles_once(
                &bot,
                &state,
                &mut applied_titles,
                &mut viewer_injections,
            )
            .await
            {
                warn!(event = "workspace_status.sync.failed", error = %error);
            }
            if let Err(error) = sync_cli_transcript_mirrors_once(
                &bot,
                &state,
                &mut workspace_event_offsets,
                &mut pending_cli_user_prompts,
            )
            .await
            {
                warn!(event = "workspace_mirror.sync.failed", error = %error);
            }
            tokio::time::sleep(Duration::from_millis(
                state.config.workspace_status_poll_interval_ms,
            ))
            .await;
        }
    });
}

async fn sync_workspace_titles_once(
    bot: &Bot,
    state: &AppState,
    applied_titles: &mut HashMap<String, String>,
    viewer_injections: &mut HashMap<String, CliViewerInjectionState>,
) -> Result<()> {
    let records = state.repository.list_active_threads().await?;
    let mut active_conversations = HashSet::new();
    let mut keep_workspaces = Vec::new();
    let mut aggregate_by_workspace: HashMap<String, WorkspaceAggregateStatus> = HashMap::new();
    let mut owner_claim_by_workspace: HashMap<String, Option<CliOwnerClaim>> = HashMap::new();

    for record in records {
        let Some(message_thread_id) = record.metadata.message_thread_id else {
            continue;
        };
        active_conversations.insert(record.conversation_key.clone());

        let mut session = state.repository.read_session_binding(&record).await?;
        let workspace_path = session
            .as_ref()
            .and_then(|binding| binding.workspace_cwd.as_deref())
            .map(PathBuf::from);

        let aggregate = if let Some(workspace_path) = workspace_path.as_ref() {
            let key = workspace_path
                .canonicalize()
                .unwrap_or_else(|_| workspace_path.clone())
                .display()
                .to_string();
            keep_workspaces.push(key.clone());
            if !owner_claim_by_workspace.contains_key(&key) {
                owner_claim_by_workspace
                    .insert(key.clone(), read_cli_owner_claim(workspace_path).await?);
            }
            if let Some(existing) = aggregate_by_workspace.get(&key) {
                Some(existing.clone())
            } else {
                let loaded =
                    crate::workspace_status::read_workspace_aggregate_status(workspace_path)
                        .await?;
                state.workspace_status_cache.insert(loaded.clone()).await;
                aggregate_by_workspace.insert(key, loaded.clone());
                Some(loaded)
            }
        } else {
            None
        };

        if let (Some(binding), Some(aggregate)) = (session.as_ref(), aggregate.as_ref())
            && binding.attachment_state == SessionAttachmentState::CliHandoff
        {
            let selected_session_id = usable_bound_session_id(session.as_ref());
            if selected_session_id.is_some_and(|session_id| {
                aggregate
                    .live_cli_session_ids
                    .iter()
                    .any(|item| item == session_id)
            }) {
                let released = state
                    .repository
                    .clear_cli_handoff_attachment(record.clone())
                    .await?;
                session = state.repository.read_session_binding(&released).await?;
            }
        }

        let owner_claim = workspace_path.as_ref().and_then(|path| {
            let key = path
                .canonicalize()
                .unwrap_or_else(|_| path.clone())
                .display()
                .to_string();
            owner_claim_by_workspace
                .get(&key)
                .and_then(|claim| claim.as_ref())
        });
        let marker =
            cli_topic_marker_for_record(&record, session.as_ref(), aggregate.as_ref(), owner_claim);
        match cli_viewer_injection_transition(
            viewer_injections.get(&record.conversation_key),
            marker,
            &record,
            owner_claim,
        ) {
            CliViewerInjectionTransition::None => {}
            CliViewerInjectionTransition::Enter(current) => {
                let text = cli_viewer_enter_message(current.shell_pid);
                if let Some(message_thread_id) = record.metadata.message_thread_id {
                    send_scoped_message(
                        bot,
                        ChatId(record.metadata.chat_id),
                        Some(thread_id_from_i32(message_thread_id)),
                        text.clone(),
                    )
                    .await?;
                }
                state
                    .repository
                    .append_log(&record, LogDirection::System, text, None)
                    .await?;
                viewer_injections.insert(record.conversation_key.clone(), current);
            }
            CliViewerInjectionTransition::Exit => {
                let text = cli_viewer_exit_message();
                if let Some(message_thread_id) = record.metadata.message_thread_id {
                    send_scoped_message(
                        bot,
                        ChatId(record.metadata.chat_id),
                        Some(thread_id_from_i32(message_thread_id)),
                        text,
                    )
                    .await?;
                }
                state
                    .repository
                    .append_log(&record, LogDirection::System, text, None)
                    .await?;
                viewer_injections.remove(&record.conversation_key);
            }
            CliViewerInjectionTransition::ExitAndEnter(current) => {
                let exit_text = cli_viewer_exit_message();
                if let Some(message_thread_id) = record.metadata.message_thread_id {
                    send_scoped_message(
                        bot,
                        ChatId(record.metadata.chat_id),
                        Some(thread_id_from_i32(message_thread_id)),
                        exit_text,
                    )
                    .await?;
                }
                state
                    .repository
                    .append_log(&record, LogDirection::System, exit_text, None)
                    .await?;
                let enter_text = cli_viewer_enter_message(current.shell_pid);
                if let Some(message_thread_id) = record.metadata.message_thread_id {
                    send_scoped_message(
                        bot,
                        ChatId(record.metadata.chat_id),
                        Some(thread_id_from_i32(message_thread_id)),
                        enter_text.clone(),
                    )
                    .await?;
                }
                state
                    .repository
                    .append_log(&record, LogDirection::System, enter_text, None)
                    .await?;
                viewer_injections.insert(record.conversation_key.clone(), current);
            }
            CliViewerInjectionTransition::ClearSilently => {
                viewer_injections.remove(&record.conversation_key);
            }
        }
        let rendered = render_topic_title(&record, workspace_path.as_deref(), marker);
        let previous = applied_titles.get(&record.conversation_key);
        if previous.is_some_and(|value| value == &rendered) {
            continue;
        }

        apply_thread_topic_title(
            bot,
            &record,
            workspace_path.as_deref(),
            message_thread_id,
            &rendered,
            "workspace_status_sync",
        )
        .await?;
        applied_titles.insert(record.conversation_key.clone(), rendered);
    }

    applied_titles.retain(|conversation, _| active_conversations.contains(conversation));
    viewer_injections.retain(|conversation, _| active_conversations.contains(conversation));
    state
        .workspace_status_cache
        .remove_missing_workspaces(&keep_workspaces)
        .await;
    Ok(())
}

async fn sync_cli_transcript_mirrors_once(
    bot: &Bot,
    state: &AppState,
    workspace_event_offsets: &mut HashMap<String, usize>,
    pending_cli_user_prompts: &mut HashSet<String>,
) -> Result<()> {
    let records = state.repository.list_active_threads().await?;
    let mut by_workspace: HashMap<String, Vec<ThreadRecord>> = HashMap::new();
    for record in records {
        if matches!(record.metadata.status, ThreadStatus::Archived) {
            continue;
        }
        let Some(binding) = state.repository.read_session_binding(&record).await? else {
            continue;
        };
        let Some(workspace_cwd) = binding.workspace_cwd else {
            continue;
        };
        by_workspace.entry(workspace_cwd).or_default().push(record);
    }

    for (workspace_key, workspace_records) in by_workspace {
        let workspace_path = PathBuf::from(&workspace_key);
        let Some(owner_claim) = read_cli_owner_claim(&workspace_path).await? else {
            pending_cli_user_prompts.retain(|key| !key.starts_with(&workspace_key));
            let Some(lines) = read_workspace_event_lines(&workspace_path).await? else {
                continue;
            };
            workspace_event_offsets.insert(workspace_key.clone(), lines.len());
            continue;
        };
        let aggregate =
            crate::workspace_status::read_workspace_aggregate_status(&workspace_path).await?;
        if workspace_cli_conflict(Some(&aggregate), Some(&owner_claim)) {
            pending_cli_user_prompts.retain(|key| !key.starts_with(&workspace_key));
            let Some(lines) = read_workspace_event_lines(&workspace_path).await? else {
                continue;
            };
            workspace_event_offsets.insert(workspace_key.clone(), lines.len());
            continue;
        }
        let Some(owner_record) = workspace_records
            .iter()
            .find(|record| record.metadata.thread_key == owner_claim.thread_key)
            .cloned()
        else {
            pending_cli_user_prompts.retain(|key| !key.starts_with(&workspace_key));
            let Some(lines) = read_workspace_event_lines(&workspace_path).await? else {
                continue;
            };
            workspace_event_offsets.insert(workspace_key.clone(), lines.len());
            continue;
        };

        let Some(lines) = read_workspace_event_lines(&workspace_path).await? else {
            continue;
        };
        let Some(previous_offset) = workspace_event_offsets.get(&workspace_key).copied() else {
            workspace_event_offsets.insert(workspace_key.clone(), lines.len());
            continue;
        };
        let new_offset = lines.len();
        for line in lines.iter().skip(previous_offset) {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let event: WorkspaceStatusEventRecord = match serde_json::from_str(trimmed) {
                Ok(event) => event,
                Err(error) => {
                    warn!(event = "workspace_mirror.event_parse_failed", error = %error);
                    continue;
                }
            };
            match event.event.as_str() {
                "user_prompt_submitted" => {
                    let Some(session_id) = event
                        .payload
                        .get("session_id")
                        .and_then(|value| value.as_str())
                    else {
                        warn!(
                            event = "workspace_mirror.cli_user_prompt_missing_session",
                            workspace = %workspace_key,
                            thread_key = %owner_record.metadata.thread_key,
                        );
                        continue;
                    };
                    if owner_claim
                        .session_id
                        .as_deref()
                        .is_some_and(|expected| expected != session_id)
                    {
                        continue;
                    }
                    let Some(entry) =
                        cli_mirror_entry_from_event(&event, owner_claim.session_id.as_deref())
                    else {
                        warn!(
                            event = "workspace_mirror.cli_user_prompt_missing_text",
                            workspace = %workspace_key,
                            thread_key = %owner_record.metadata.thread_key,
                            session_id = session_id,
                        );
                        continue;
                    };
                    pending_cli_user_prompts
                        .insert(cli_prompt_tracking_key(&workspace_key, &entry.session_id));
                    state
                        .repository
                        .append_transcript_mirror(&owner_record, &entry)
                        .await?;
                    if let Some(message_thread_id) = owner_record.metadata.message_thread_id {
                        send_scoped_message(
                            bot,
                            ChatId(owner_record.metadata.chat_id),
                            Some(thread_id_from_i32(message_thread_id)),
                            format!("CLI: {}", entry.text),
                        )
                        .await?;
                    }
                    continue;
                }
                "turn_completed" => {
                    if let Some(session_id) = event
                        .payload
                        .get("thread-id")
                        .and_then(|value| value.as_str())
                    {
                        if owner_claim
                            .session_id
                            .as_deref()
                            .is_none_or(|expected| expected == session_id)
                            && !pending_cli_user_prompts
                                .remove(&cli_prompt_tracking_key(&workspace_key, session_id))
                        {
                            warn!(
                                event = "workspace_mirror.cli_user_prompt_missing",
                                workspace = %workspace_key,
                                thread_key = %owner_record.metadata.thread_key,
                                session_id = session_id,
                            );
                        }
                    }
                }
                _ => {}
            }
            if let Some(entry) =
                cli_mirror_entry_from_event(&event, owner_claim.session_id.as_deref())
            {
                state
                    .repository
                    .append_transcript_mirror(&owner_record, &entry)
                    .await?;
                if let Some(message_thread_id) = owner_record.metadata.message_thread_id {
                    let prefix = match (entry.origin.clone(), entry.role.clone()) {
                        (TranscriptMirrorOrigin::Cli, TranscriptMirrorRole::User) => "CLI",
                        (TranscriptMirrorOrigin::Cli, TranscriptMirrorRole::Assistant) => "Codex",
                        _ => continue,
                    };
                    send_scoped_message(
                        bot,
                        ChatId(owner_record.metadata.chat_id),
                        Some(thread_id_from_i32(message_thread_id)),
                        format!("{prefix}: {}", entry.text),
                    )
                    .await?;
                }
            }
        }
        workspace_event_offsets.insert(workspace_key, new_offset);
    }
    Ok(())
}

fn cli_prompt_tracking_key(workspace_key: &str, session_id: &str) -> String {
    format!("{workspace_key}::{session_id}")
}

async fn read_workspace_event_lines(workspace_path: &Path) -> Result<Option<Vec<String>>> {
    let path = events_path(workspace_path);
    let content = match tokio::fs::read_to_string(&path).await {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    Ok(Some(content.lines().map(str::to_owned).collect()))
}

fn cli_mirror_entry_from_event(
    event: &WorkspaceStatusEventRecord,
    expected_session_id: Option<&str>,
) -> Option<TranscriptMirrorEntry> {
    match event.event.as_str() {
        "user_prompt_submitted" => {
            let session_id = event.payload.get("session_id")?.as_str()?;
            if expected_session_id.is_some_and(|expected| expected != session_id) {
                return None;
            }
            let text = event.payload.get("prompt")?.as_str()?.trim();
            if text.is_empty() {
                return None;
            }
            Some(TranscriptMirrorEntry {
                timestamp: event.occurred_at.clone(),
                session_id: session_id.to_owned(),
                origin: TranscriptMirrorOrigin::Cli,
                role: TranscriptMirrorRole::User,
                delivery: TranscriptMirrorDelivery::Final,
                text: text.to_owned(),
            })
        }
        "turn_completed" => {
            let session_id = event.payload.get("thread-id")?.as_str()?;
            if expected_session_id.is_some_and(|expected| expected != session_id) {
                return None;
            }
            let text = event
                .payload
                .get("last-assistant-message")?
                .as_str()?
                .trim();
            if text.is_empty() {
                return None;
            }
            Some(TranscriptMirrorEntry {
                timestamp: event.occurred_at.clone(),
                session_id: session_id.to_owned(),
                origin: TranscriptMirrorOrigin::Cli,
                role: TranscriptMirrorRole::Assistant,
                delivery: TranscriptMirrorDelivery::Final,
                text: text.to_owned(),
            })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CliTopicMarker, CliViewerInjectionState, CliViewerInjectionTransition,
        STARTUP_STALE_BUSY_RECOVERED_LOG, cli_mirror_entry_from_event, cli_topic_marker_for_record,
        cli_viewer_injection_transition, reconcile_stale_bot_busy_sessions_for_repository,
        render_topic_title,
    };
    use crate::repository::{
        SessionAttachmentState, SessionBinding, ThreadMetadata, ThreadRecord, ThreadRepository,
        ThreadScope, ThreadStatus, TranscriptMirrorOrigin, TranscriptMirrorRole,
    };
    use crate::workspace_status::{
        CliOwnerClaim, SessionCurrentStatus, SessionStatusOwner, WorkspaceAggregateStatus,
        WorkspaceStatusEventRecord, WorkspaceStatusPhase, ensure_workspace_status_surface,
        read_session_status, record_bot_status_event, session_status_path,
    };
    use serde_json::json;
    use std::path::PathBuf;
    use tokio::fs;
    use uuid::Uuid;

    fn temp_path() -> PathBuf {
        std::env::temp_dir().join(format!("threadbridge-status-sync-test-{}", Uuid::new_v4()))
    }

    async fn setup_repo_and_workspace(
        workspace_name: &str,
    ) -> (ThreadRepository, PathBuf, PathBuf, ThreadRecord) {
        let root = temp_path();
        let data_root = root.join("data");
        let workspace = root.join(workspace_name);
        fs::create_dir_all(&workspace).await.unwrap();
        let repo = ThreadRepository::open(&data_root).await.unwrap();
        let record = repo
            .create_thread(1, 100, "status".to_owned())
            .await
            .unwrap();
        let record = repo
            .bind_workspace(
                record,
                workspace.display().to_string(),
                "thr_test".to_owned(),
            )
            .await
            .unwrap();
        (repo, root, workspace, record)
    }

    fn record(title: Option<&str>, session_broken: bool) -> ThreadRecord {
        ThreadRecord {
            conversation_key: "thread:test".to_owned(),
            folder_name: "folder".to_owned(),
            folder_path: PathBuf::from("/tmp/folder"),
            log_path: PathBuf::from("/tmp/folder/conversations.jsonl"),
            metadata_path: PathBuf::from("/tmp/folder/metadata.json"),
            metadata: ThreadMetadata {
                archived_at: None,
                chat_id: 1,
                created_at: "2026-03-19T00:00:00.000Z".to_owned(),
                last_codex_turn_at: None,
                message_thread_id: Some(123),
                previous_message_thread_ids: Vec::new(),
                scope: ThreadScope::Thread,
                session_broken,
                session_broken_at: None,
                session_broken_reason: None,
                status: ThreadStatus::Active,
                title: title.map(str::to_owned),
                updated_at: "2026-03-19T00:00:00.000Z".to_owned(),
                thread_key: "thread-key".to_owned(),
            },
        }
    }

    fn aggregate(session_ids: &[&str]) -> WorkspaceAggregateStatus {
        WorkspaceAggregateStatus {
            schema_version: 2,
            workspace_cwd: "/tmp/workspace".to_owned(),
            live_cli_session_ids: session_ids
                .iter()
                .map(|value| (*value).to_owned())
                .collect(),
            active_shell_pids: vec![42],
            updated_at: "2026-03-19T00:00:00.000Z".to_owned(),
        }
    }

    fn owner_claim(thread_key: &str, session_id: Option<&str>) -> CliOwnerClaim {
        CliOwnerClaim {
            schema_version: 2,
            workspace_cwd: "/tmp/workspace".to_owned(),
            thread_key: thread_key.to_owned(),
            shell_pid: 42,
            session_id: session_id.map(str::to_owned),
            child_pid: None,
            child_pgid: None,
            child_command: None,
            started_at: "2026-03-19T00:00:00.000Z".to_owned(),
            updated_at: "2026-03-19T00:00:00.000Z".to_owned(),
        }
    }

    fn binding(
        selected_session_id: Option<&str>,
        attachment_state: SessionAttachmentState,
    ) -> SessionBinding {
        SessionBinding {
            schema_version: 2,
            codex_thread_id: selected_session_id.map(str::to_owned),
            selected_session_id: selected_session_id.map(str::to_owned),
            attachment_state,
            workspace_cwd: Some("/tmp/workspace".to_owned()),
            bound_at: None,
            initialized_at: None,
            last_verified_at: None,
            session_broken: false,
            session_broken_at: None,
            session_broken_reason: None,
            updated_at: "2026-03-19T00:00:00.000Z".to_owned(),
        }
    }

    #[test]
    fn render_title_uses_attach_for_cli_handoff_binding() {
        let title = render_topic_title(
            &record(Some("Status Sync"), true),
            Some(PathBuf::from("/tmp/workspace").as_path()),
            CliTopicMarker::Attach,
        );
        assert_eq!(title, "Status Sync · attach · broken");
    }

    #[test]
    fn render_title_uses_cli_for_owner_thread() {
        let marker = cli_topic_marker_for_record(
            &record(None, false),
            Some(&binding(Some("thr_bot"), SessionAttachmentState::None)),
            Some(&aggregate(&["thr_cli"])),
            Some(&owner_claim("thread-key", Some("thr_cli"))),
        );
        let title = render_topic_title(
            &record(None, false),
            Some(PathBuf::from("/tmp/example-workspace").as_path()),
            marker,
        );
        assert_eq!(title, "example-workspace · cli");
    }

    #[test]
    fn render_title_truncates_before_attach_suffix() {
        let long_title = "x".repeat(140);
        let title = render_topic_title(
            &record(Some(&long_title), false),
            Some(PathBuf::from("/tmp/workspace").as_path()),
            CliTopicMarker::Attach,
        );
        assert!(title.ends_with(" · attach"));
        assert!(title.chars().count() <= 128);
    }

    #[test]
    fn cli_conflict_marker_appears_when_live_cli_has_no_owner_claim() {
        let marker = cli_topic_marker_for_record(
            &record(Some("Conflict"), false),
            Some(&binding(Some("thr_bot"), SessionAttachmentState::None)),
            Some(&aggregate(&["thr_cli"])),
            None,
        );
        let title = render_topic_title(
            &record(Some("Conflict"), false),
            Some(PathBuf::from("/tmp/workspace").as_path()),
            marker,
        );
        assert_eq!(title, "Conflict · cli!");
    }

    #[test]
    fn cli_user_prompt_event_creates_cli_user_entry() {
        let event = WorkspaceStatusEventRecord {
            schema_version: 2,
            event: "user_prompt_submitted".to_owned(),
            source: crate::workspace_status::SessionStatusOwner::Cli,
            workspace_cwd: "/tmp/workspace".to_owned(),
            occurred_at: "2026-03-19T00:00:00.000Z".to_owned(),
            payload: json!({
                "session_id": "thr_cli",
                "prompt": "inspect this repo"
            }),
        };
        let entry = cli_mirror_entry_from_event(&event, Some("thr_cli")).expect("cli user entry");
        assert_eq!(entry.origin, TranscriptMirrorOrigin::Cli);
        assert_eq!(entry.role, TranscriptMirrorRole::User);
        assert_eq!(entry.text, "inspect this repo");
    }

    #[test]
    fn cli_viewer_transition_enters_for_owner_thread_cli_marker() {
        let record = record(Some("Viewer"), false);
        let transition = cli_viewer_injection_transition(
            None,
            CliTopicMarker::Cli,
            &record,
            Some(&owner_claim("thread-key", Some("thr_cli"))),
        );
        assert_eq!(
            transition,
            CliViewerInjectionTransition::Enter(CliViewerInjectionState {
                lifecycle_id: "thread-key:42:2026-03-19T00:00:00.000Z".to_owned(),
                shell_pid: 42,
            })
        );
    }

    #[test]
    fn cli_viewer_transition_exits_when_marker_returns_to_none() {
        let record = record(Some("Viewer"), false);
        let previous = CliViewerInjectionState {
            lifecycle_id: "thread-key:42:2026-03-19T00:00:00.000Z".to_owned(),
            shell_pid: 42,
        };
        let transition =
            cli_viewer_injection_transition(Some(&previous), CliTopicMarker::None, &record, None);
        assert_eq!(transition, CliViewerInjectionTransition::Exit);
    }

    #[test]
    fn cli_viewer_transition_clears_silently_on_attach() {
        let record = record(Some("Viewer"), false);
        let previous = CliViewerInjectionState {
            lifecycle_id: "thread-key:42:2026-03-19T00:00:00.000Z".to_owned(),
            shell_pid: 42,
        };
        let transition = cli_viewer_injection_transition(
            Some(&previous),
            CliTopicMarker::Attach,
            &record,
            Some(&owner_claim("thread-key", Some("thr_cli"))),
        );
        assert_eq!(transition, CliViewerInjectionTransition::ClearSilently);
    }

    #[test]
    fn cli_user_prompt_event_without_prompt_is_ignored() {
        let event = WorkspaceStatusEventRecord {
            schema_version: 2,
            event: "user_prompt_submitted".to_owned(),
            source: crate::workspace_status::SessionStatusOwner::Cli,
            workspace_cwd: "/tmp/workspace".to_owned(),
            occurred_at: "2026-03-19T00:00:00.000Z".to_owned(),
            payload: json!({
                "session_id": "thr_cli"
            }),
        };
        assert!(cli_mirror_entry_from_event(&event, Some("thr_cli")).is_none());
    }

    #[test]
    fn turn_completed_does_not_fallback_to_input_messages_for_cli_user_entry() {
        let event = WorkspaceStatusEventRecord {
            schema_version: 2,
            event: "turn_completed".to_owned(),
            source: crate::workspace_status::SessionStatusOwner::Cli,
            workspace_cwd: "/tmp/workspace".to_owned(),
            occurred_at: "2026-03-19T00:00:00.000Z".to_owned(),
            payload: json!({
                "thread-id": "thr_cli",
                "input-messages": ["hello from cli"]
            }),
        };
        assert!(cli_mirror_entry_from_event(&event, Some("thr_cli")).is_none());
    }

    #[tokio::test]
    async fn startup_reconciliation_recovers_bot_busy_session() {
        let (repo, root, workspace, record) = setup_repo_and_workspace("workspace").await;
        record_bot_status_event(
            &workspace,
            "bot_turn_started",
            Some("thr_test"),
            Some("turn-1"),
            Some("hello"),
        )
        .await
        .unwrap();

        let report = reconcile_stale_bot_busy_sessions_for_repository(&repo)
            .await
            .unwrap();
        assert_eq!(report.scanned_threads, 1);
        assert_eq!(report.unique_sessions, 1);
        assert_eq!(report.recovered_sessions, 1);
        assert_eq!(report.recovered_threads, 1);
        assert_eq!(report.skipped_threads, 0);

        let session = read_session_status(&workspace, "thr_test")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(session.owner, SessionStatusOwner::Bot);
        assert_eq!(session.phase, WorkspaceStatusPhase::Idle);
        assert_eq!(session.turn_id, None);

        let log = fs::read_to_string(&record.log_path).await.unwrap();
        assert!(log.contains(STARTUP_STALE_BUSY_RECOVERED_LOG));

        let _ = fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn startup_reconciliation_does_not_recover_cli_busy_session() {
        let (repo, root, workspace, record) = setup_repo_and_workspace("workspace").await;
        ensure_workspace_status_surface(&workspace).await.unwrap();
        let cli_session = SessionCurrentStatus {
            schema_version: 2,
            workspace_cwd: workspace.display().to_string(),
            session_id: "thr_test".to_owned(),
            owner: SessionStatusOwner::Cli,
            live: true,
            phase: WorkspaceStatusPhase::TurnRunning,
            shell_pid: Some(42),
            child_pid: None,
            child_pgid: None,
            child_command: None,
            client: Some("codex-cli".to_owned()),
            turn_id: Some("turn-1".to_owned()),
            summary: Some("cli run".to_owned()),
            updated_at: "2026-03-19T00:00:00.000Z".to_owned(),
        };
        fs::write(
            session_status_path(&workspace, "thr_test"),
            format!("{}\n", serde_json::to_string_pretty(&cli_session).unwrap()),
        )
        .await
        .unwrap();

        let report = reconcile_stale_bot_busy_sessions_for_repository(&repo)
            .await
            .unwrap();
        assert_eq!(report.recovered_sessions, 0);
        assert_eq!(report.recovered_threads, 0);
        assert_eq!(report.skipped_threads, 1);

        let session = read_session_status(&workspace, "thr_test")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(session.owner, SessionStatusOwner::Cli);
        assert_eq!(session.phase, WorkspaceStatusPhase::TurnRunning);

        let log = fs::read_to_string(&record.log_path)
            .await
            .unwrap_or_default();
        assert!(!log.contains(STARTUP_STALE_BUSY_RECOVERED_LOG));

        let _ = fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn startup_reconciliation_recovers_shared_session_once_and_logs_all_threads() {
        let root = temp_path();
        let data_root = root.join("data");
        let workspace = root.join("workspace");
        fs::create_dir_all(&workspace).await.unwrap();
        let repo = ThreadRepository::open(&data_root).await.unwrap();

        let record_a = repo.create_thread(1, 100, "A".to_owned()).await.unwrap();
        let record_a = repo
            .bind_workspace(
                record_a,
                workspace.display().to_string(),
                "thr_shared".to_owned(),
            )
            .await
            .unwrap();
        let record_b = repo.create_thread(1, 101, "B".to_owned()).await.unwrap();
        let record_b = repo
            .bind_workspace(
                record_b,
                workspace.display().to_string(),
                "thr_shared".to_owned(),
            )
            .await
            .unwrap();

        record_bot_status_event(
            &workspace,
            "bot_turn_started",
            Some("thr_shared"),
            Some("turn-1"),
            Some("pending"),
        )
        .await
        .unwrap();

        let report = reconcile_stale_bot_busy_sessions_for_repository(&repo)
            .await
            .unwrap();
        assert_eq!(report.scanned_threads, 2);
        assert_eq!(report.unique_sessions, 1);
        assert_eq!(report.recovered_sessions, 1);
        assert_eq!(report.recovered_threads, 2);
        assert_eq!(report.skipped_threads, 0);

        let log_a = fs::read_to_string(&record_a.log_path).await.unwrap();
        let log_b = fs::read_to_string(&record_b.log_path).await.unwrap();
        assert!(log_a.contains(STARTUP_STALE_BUSY_RECOVERED_LOG));
        assert!(log_b.contains(STARTUP_STALE_BUSY_RECOVERED_LOG));

        let _ = fs::remove_dir_all(root).await;
    }
}
