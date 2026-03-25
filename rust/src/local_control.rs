use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use teloxide::prelude::*;
use teloxide::types::{MessageId, ThreadId};

use crate::repository::{LogDirection, ThreadRecord};
use crate::runtime_control::{RuntimeControlContext, WorkspaceAddResolution};
use crate::telegram_runtime::{AppState, send_scoped_message, status_sync, thread_id_to_i32};

#[derive(Clone)]
pub struct LocalControlHandle {
    bot: Bot,
    control: RuntimeControlContext,
}

#[derive(Debug, Clone)]
pub struct CreatedThread {
    pub record: ThreadRecord,
    pub title: String,
}

#[derive(Debug, Clone)]
pub enum AddWorkspaceOutcome {
    Created(ThreadRecord),
    Existing(ThreadRecord),
}

impl AddWorkspaceOutcome {
    pub fn record(&self) -> &ThreadRecord {
        match self {
            Self::Created(record) | Self::Existing(record) => record,
        }
    }

    pub fn created(&self) -> bool {
        matches!(self, Self::Created(_))
    }
}

impl LocalControlHandle {
    pub fn new(bot: Bot, state: AppState) -> Self {
        Self {
            bot,
            control: state.control.clone(),
        }
    }

    pub async fn create_thread(&self, title: Option<String>) -> Result<CreatedThread> {
        let main_thread = self
            .control
            .repository
            .find_main_thread()
            .await?
            .context("Control chat is not ready yet. Send /start to the bot from the target Telegram chat first.")?;
        let title = title
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| format!("Thread {}", chrono::Local::now().format("%m-%d %H:%M")));
        let topic = self
            .bot
            .create_forum_topic(ChatId(main_thread.metadata.chat_id), title.clone())
            .await?;
        let record = self
            .control
            .repository
            .create_thread(
                main_thread.metadata.chat_id,
                thread_id_to_i32(topic.thread_id),
                topic.name.clone(),
            )
            .await?;
        self.control
            .repository
            .append_log(
                &record,
                LogDirection::System,
                "Telegram thread created from local management UI.",
                None,
            )
            .await?;
        send_scoped_message(
            &self.bot,
            ChatId(main_thread.metadata.chat_id),
            None,
            format!("Created thread \"{}\".", topic.name),
        )
        .await?;
        send_scoped_message(
            &self.bot,
            ChatId(main_thread.metadata.chat_id),
            Some(topic.thread_id),
            "Thread created from local management UI.",
        )
        .await?;
        Ok(CreatedThread {
            record,
            title: topic.name,
        })
    }

    pub async fn create_thread_and_bind(
        &self,
        title: Option<String>,
        workspace_cwd: &str,
    ) -> Result<ThreadRecord> {
        let created = self.create_thread(title).await?;
        self.bind_workspace(&created.record.metadata.thread_key, workspace_cwd)
            .await
    }

    pub async fn add_workspace(&self, workspace_cwd: &str) -> Result<AddWorkspaceOutcome> {
        let workspace_path = resolve_workspace_argument(workspace_cwd).await?;
        match self
            .control
            .workspace_session_service()
            .resolve_workspace_add(&workspace_path)
            .await?
        {
            WorkspaceAddResolution::Existing(record) => Ok(AddWorkspaceOutcome::Existing(record)),
            WorkspaceAddResolution::Create {
                canonical_workspace_cwd,
                suggested_title,
            } => {
                let record = self
                    .create_thread_and_bind(Some(suggested_title), &canonical_workspace_cwd)
                    .await?;
                Ok(AddWorkspaceOutcome::Created(record))
            }
        }
    }

    pub async fn bind_workspace(
        &self,
        thread_key: &str,
        workspace_cwd: &str,
    ) -> Result<ThreadRecord> {
        let record = self
            .control
            .repository
            .find_active_thread_by_key(thread_key)
            .await?
            .context("thread_key is not an active thread")?;
        let workspace_path = resolve_workspace_argument(workspace_cwd).await?;
        let updated = self
            .control
            .workspace_session_service()
            .bind_workspace_record(record, &workspace_path)
            .await?;
        self.control
            .repository
            .append_log(
                &updated,
                LogDirection::System,
                format!(
                    "Bound Telegram thread to workspace {} from local management UI.",
                    workspace_path.display()
                ),
                None,
            )
            .await?;
        if let Some(message_thread_id) = updated.metadata.message_thread_id {
            send_scoped_message(
                &self.bot,
                ChatId(updated.metadata.chat_id),
                Some(ThreadId(MessageId(message_thread_id))),
                format!(
                    "Bound workspace: `{}`\n\nFor the managed local TUI path in this workspace, run:\n`{}/.threadbridge/bin/hcodex`",
                    workspace_path.display(),
                    workspace_path.display()
                ),
            )
            .await?;
            let _ = status_sync::refresh_thread_topic_title(
                &self.bot,
                &self.control.repository,
                &updated,
                "local_bind_workspace",
            )
            .await;
        }
        Ok(updated)
    }

    pub async fn repair_session_binding(&self, thread_key: &str) -> Result<ThreadRecord> {
        let record = self
            .control
            .repository
            .find_active_thread_by_key(thread_key)
            .await?
            .context("thread_key is not an active thread")?;
        let session = self
            .control
            .repository
            .read_session_binding(&record)
            .await?;
        let Some(binding) = session.as_ref() else {
            bail!("This thread is not bound to a workspace yet.");
        };
        let result = self
            .control
            .workspace_session_service()
            .repair_session_binding(record, binding)
            .await?;
        self.control
            .repository
            .append_log(
                &result.record,
                LogDirection::System,
                if result.verified {
                    "Codex session revalidated from local management UI."
                } else {
                    "Codex session revalidation failed from local management UI."
                },
                None,
            )
            .await?;
        if result.verified {
            Ok(result.record)
        } else {
            bail!("Codex session repair failed. Use New Session first.")
        }
    }

    pub async fn archive_thread(&self, thread_key: &str) -> Result<ThreadRecord> {
        let record = self
            .control
            .repository
            .find_active_thread_by_key(thread_key)
            .await?
            .context("thread_key is not an active thread")?;
        if let Some(thread_id) = record.metadata.message_thread_id {
            let _ = self
                .bot
                .delete_forum_topic(
                    ChatId(record.metadata.chat_id),
                    ThreadId(MessageId(thread_id)),
                )
                .await;
        }
        let archived = self.control.repository.archive_thread(record).await?;
        self.control
            .repository
            .append_log(
                &archived,
                LogDirection::System,
                "Thread archived from local management UI.",
                None,
            )
            .await?;
        Ok(archived)
    }

    pub async fn adopt_tui_session(&self, thread_key: &str) -> Result<ThreadRecord> {
        let record = self
            .control
            .repository
            .find_active_thread_by_key(thread_key)
            .await?
            .context("thread_key is not an active thread")?;
        let updated = self
            .control
            .repository
            .adopt_tui_active_session(record)
            .await?;
        self.control
            .repository
            .append_log(
                &updated,
                LogDirection::System,
                "Adopted the active TUI session from local management UI.",
                None,
            )
            .await?;
        let _ = status_sync::refresh_thread_topic_title(
            &self.bot,
            &self.control.repository,
            &updated,
            "local_tui_adopt_accept",
        )
        .await;
        Ok(updated)
    }

    pub async fn reject_tui_session(&self, thread_key: &str) -> Result<ThreadRecord> {
        let record = self
            .control
            .repository
            .find_active_thread_by_key(thread_key)
            .await?
            .context("thread_key is not an active thread")?;
        let updated = self
            .control
            .repository
            .clear_tui_adoption_state(record)
            .await?;
        self.control
            .repository
            .append_log(
                &updated,
                LogDirection::System,
                "Rejected the active TUI session from local management UI.",
                None,
            )
            .await?;
        let _ = status_sync::refresh_thread_topic_title(
            &self.bot,
            &self.control.repository,
            &updated,
            "local_tui_adopt_reject",
        )
        .await;
        Ok(updated)
    }

    pub async fn restore_thread(&self, thread_key: &str) -> Result<ThreadRecord> {
        let archived = self
            .control
            .repository
            .get_thread_by_key(self.control_chat_id().await?, thread_key)
            .await?
            .context("thread_key is not a known thread")?;
        if !matches!(
            archived.metadata.status,
            crate::repository::ThreadStatus::Archived
        ) {
            bail!("thread_key is not archived");
        }
        let topic = self
            .bot
            .create_forum_topic(
                ChatId(archived.metadata.chat_id),
                restored_thread_title(
                    archived.metadata.title.as_deref(),
                    archived.metadata.message_thread_id,
                ),
            )
            .await?;
        let restored = self
            .control
            .repository
            .restore_thread(
                archived,
                thread_id_to_i32(topic.thread_id),
                topic.name.clone(),
            )
            .await?;
        self.control
            .repository
            .append_log(
                &restored,
                LogDirection::System,
                format!(
                    "Thread restored from local management UI into Telegram thread \"{}\" (message_thread_id {}).",
                    topic.name,
                    thread_id_to_i32(topic.thread_id)
                ),
                None,
            )
            .await?;
        send_scoped_message(
            &self.bot,
            ChatId(restored.metadata.chat_id),
            None,
            format!("Restored into \"{}\". Continue there.", topic.name),
        )
        .await?;
        send_scoped_message(
            &self.bot,
            ChatId(restored.metadata.chat_id),
            Some(topic.thread_id),
            "This thread has been restored from archive.",
        )
        .await?;
        let _ = status_sync::refresh_thread_topic_title(
            &self.bot,
            &self.control.repository,
            &restored,
            "local_restore",
        )
        .await;
        Ok(restored)
    }

    async fn control_chat_id(&self) -> Result<i64> {
        Ok(self
            .control
            .repository
            .find_main_thread()
            .await?
            .context("Control chat is not ready yet. Send /start to the bot from the target Telegram chat first.")?
            .metadata
            .chat_id)
    }
}

async fn resolve_workspace_argument(raw: &str) -> Result<PathBuf> {
    let input = PathBuf::from(raw.trim());
    if !input.is_absolute() {
        bail!("Workspace path must be absolute.");
    }
    let metadata = tokio::fs::metadata(&input)
        .await
        .with_context(|| format!("workspace path does not exist: {}", input.display()))?;
    if !metadata.is_dir() {
        bail!("Workspace path must point to a directory.");
    }
    Ok(input.canonicalize().unwrap_or(input))
}

fn restored_thread_title(title: Option<&str>, fallback_thread_id: Option<i32>) -> String {
    let base = title
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("Thread {}", fallback_thread_id.unwrap_or_default()));
    format!("{base} · 已恢復")
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::runtime_control::workspace_thread_title;

    #[test]
    fn workspace_thread_title_prefers_folder_name() {
        let title = workspace_thread_title(Path::new("/tmp/threadBridge/workspaces/Trackly"));
        assert_eq!(title, "Trackly");
    }

    #[test]
    fn workspace_thread_title_falls_back_to_full_path() {
        let title = workspace_thread_title(Path::new("/"));
        assert_eq!(title, "/");
    }
}
