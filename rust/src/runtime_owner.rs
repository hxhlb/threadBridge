use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;
use chrono::Utc;
use serde::Serialize;
use tokio::sync::RwLock;

use crate::app_server_runtime::WorkspaceRuntimeManager;
use crate::config::RuntimeConfig;
use crate::repository::ThreadRepository;
use crate::tui_proxy::TuiProxyManager;
use crate::workspace::{ensure_workspace_runtime, validate_seed_template};

#[derive(Debug, Clone, Default, Serialize)]
pub struct RuntimeOwnerReconcileReport {
    pub scanned_workspaces: usize,
    pub ensured_workspaces: usize,
    pub ensured_proxies: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeOwnerStatus {
    pub state: &'static str,
    pub last_reconcile_started_at: Option<String>,
    pub last_reconcile_finished_at: Option<String>,
    pub last_successful_reconcile_at: Option<String>,
    pub last_error: Option<String>,
    pub last_report: RuntimeOwnerReconcileReport,
}

impl RuntimeOwnerStatus {
    pub fn inactive() -> Self {
        Self {
            state: "inactive",
            last_reconcile_started_at: None,
            last_reconcile_finished_at: None,
            last_successful_reconcile_at: None,
            last_error: None,
            last_report: RuntimeOwnerReconcileReport::default(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct DesktopRuntimeOwner {
    runtime: RuntimeConfig,
    seed_template_path: PathBuf,
    app_server_runtime: WorkspaceRuntimeManager,
    tui_proxy_runtime: TuiProxyManager,
    status: Arc<RwLock<RuntimeOwnerStatus>>,
}

impl DesktopRuntimeOwner {
    pub async fn new(runtime: RuntimeConfig) -> Result<Self> {
        let seed_template_path = validate_seed_template(
            &runtime
                .codex_working_directory
                .join("templates")
                .join("AGENTS.md"),
        )?;
        let repository = ThreadRepository::open(&runtime.data_root_path).await?;
        Ok(Self {
            runtime,
            seed_template_path,
            app_server_runtime: WorkspaceRuntimeManager::new(),
            tui_proxy_runtime: TuiProxyManager::new(repository),
            status: Arc::new(RwLock::new(RuntimeOwnerStatus {
                state: "idle",
                last_reconcile_started_at: None,
                last_reconcile_finished_at: None,
                last_successful_reconcile_at: None,
                last_error: None,
                last_report: RuntimeOwnerReconcileReport::default(),
            })),
        })
    }

    pub async fn status(&self) -> RuntimeOwnerStatus {
        self.status.read().await.clone()
    }

    pub async fn reconcile_managed_workspaces<I, S>(
        &self,
        workspaces: I,
    ) -> Result<RuntimeOwnerReconcileReport>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let unique_workspaces = workspaces
            .into_iter()
            .map(|workspace| canonical_workspace_string(Path::new(workspace.as_ref())))
            .collect::<BTreeSet<_>>();
        let mut report = RuntimeOwnerReconcileReport {
            scanned_workspaces: unique_workspaces.len(),
            ensured_workspaces: 0,
            ensured_proxies: 0,
        };
        let started_at = now_iso();
        {
            let mut status = self.status.write().await;
            status.state = "running";
            status.last_reconcile_started_at = Some(started_at);
            status.last_error = None;
        }
        for workspace in unique_workspaces {
            let workspace_path = Path::new(&workspace);
            let step = async {
                ensure_workspace_runtime(
                    &self.runtime.codex_working_directory,
                    &self.runtime.data_root_path,
                    &self.seed_template_path,
                    workspace_path,
                )
                .await?;
                let runtime = self
                    .app_server_runtime
                    .ensure_workspace_daemon(workspace_path)
                    .await?;
                let _ = self
                    .tui_proxy_runtime
                    .ensure_workspace_proxy(workspace_path, &runtime.daemon_ws_url)
                    .await?;
                Ok::<(), anyhow::Error>(())
            }
            .await;
            if let Err(error) = step {
                let finished_at = now_iso();
                let mut status = self.status.write().await;
                status.state = "error";
                status.last_reconcile_finished_at = Some(finished_at);
                status.last_error = Some(error.to_string());
                status.last_report = report.clone();
                return Err(error);
            }
            report.ensured_workspaces += 1;
            report.ensured_proxies += 1;
        }
        let finished_at = now_iso();
        let mut status = self.status.write().await;
        status.state = "healthy";
        status.last_reconcile_finished_at = Some(finished_at.clone());
        status.last_successful_reconcile_at = Some(finished_at);
        status.last_error = None;
        status.last_report = report.clone();
        Ok(report)
    }
}

fn canonical_workspace_string(workspace_path: &Path) -> String {
    workspace_path
        .canonicalize()
        .unwrap_or_else(|_| workspace_path.to_path_buf())
        .display()
        .to_string()
}

fn now_iso() -> String {
    Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}
