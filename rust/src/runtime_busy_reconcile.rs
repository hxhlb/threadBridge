use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::Result;
use tracing::info;

use crate::repository::{LogDirection, SessionBinding, ThreadRepository};
use crate::thread_state::{BindingStatus, resolve_binding_status};
use crate::workspace_status::{
    SessionActivitySource, SessionCurrentStatus, read_session_status, record_bot_status_event,
};

pub(crate) const STARTUP_STALE_BUSY_RECOVERED_LOG: &str =
    "Recovered stale busy state from previous threadBridge process during startup.";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StaleBusyReconciliationReport {
    pub scanned_threads: usize,
    pub unique_sessions: usize,
    pub recovered_sessions: usize,
    pub recovered_threads: usize,
    pub skipped_threads: usize,
}

fn current_bound_session_id(binding: &SessionBinding) -> Option<&str> {
    binding
        .current_codex_thread_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
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
    snapshot.activity_source == SessionActivitySource::ManagedRuntime
        && snapshot.phase.is_turn_busy()
}

pub async fn reconcile_stale_bot_busy_sessions(
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
        if resolve_binding_status(&record.metadata, Some(&binding)) != BindingStatus::Healthy {
            report.skipped_threads += 1;
            continue;
        }
        let Some(session_id) = current_bound_session_id(&binding) else {
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
