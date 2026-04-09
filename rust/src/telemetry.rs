use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use tokio::fs::OpenOptions;
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;
use tracing::{info, warn};

const DEFAULT_RECENT_EVENT_LIMIT: usize = 500;
const DEFAULT_SLOW_EVENT_THRESHOLD_MS: u64 = 50;

pub type RuntimeTelemetryFields = BTreeMap<String, String>;
pub type RuntimeTelemetryMetrics = BTreeMap<String, u64>;

#[derive(Debug, Clone)]
pub struct RuntimeTelemetryHandle {
    inner: Arc<RuntimeTelemetryInner>,
}

#[derive(Debug)]
struct RuntimeTelemetryInner {
    path: PathBuf,
    recent: Mutex<VecDeque<RuntimeTelemetryEvent>>,
    recent_limit: usize,
    slow_event_threshold_ms: u64,
    writer: mpsc::UnboundedSender<RuntimeTelemetryEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeTelemetryEvent {
    pub recorded_at: String,
    pub operation: String,
    pub status: String,
    pub elapsed_ms: u64,
    pub slow: bool,
    #[serde(default)]
    pub fields: RuntimeTelemetryFields,
    #[serde(default)]
    pub metrics: RuntimeTelemetryMetrics,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeTelemetrySummaryItem {
    pub operation: String,
    pub count: usize,
    pub slow_count: usize,
    pub error_count: usize,
    pub total_elapsed_ms: u64,
    pub average_elapsed_ms: u64,
    pub max_elapsed_ms: u64,
    pub last_elapsed_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeTelemetrySnapshot {
    pub path: String,
    pub retained_event_count: usize,
    pub returned_event_count: usize,
    pub slow_event_threshold_ms: u64,
    pub events: Vec<RuntimeTelemetryEvent>,
    pub summary: Vec<RuntimeTelemetrySummaryItem>,
}

#[derive(Default)]
struct RuntimeTelemetrySummaryAccumulator {
    count: usize,
    slow_count: usize,
    error_count: usize,
    total_elapsed_ms: u64,
    max_elapsed_ms: u64,
    last_elapsed_ms: u64,
}

impl RuntimeTelemetryHandle {
    pub fn new(path: PathBuf) -> Self {
        let (writer, mut receiver) = mpsc::unbounded_channel::<RuntimeTelemetryEvent>();
        let writer_path = path.clone();
        tokio::spawn(async move {
            while let Some(event) = receiver.recv().await {
                if let Err(error) = append_event(&writer_path, &event).await {
                    warn!(
                        event = "runtime_telemetry.append_failed",
                        path = %writer_path.display(),
                        error = %error,
                        "failed to append runtime telemetry event"
                    );
                }
            }
        });

        info!(
            event = "runtime_telemetry.started",
            path = %path.display(),
            retained_event_limit = DEFAULT_RECENT_EVENT_LIMIT,
            slow_event_threshold_ms = DEFAULT_SLOW_EVENT_THRESHOLD_MS,
            "runtime telemetry enabled"
        );

        Self {
            inner: Arc::new(RuntimeTelemetryInner {
                path,
                recent: Mutex::new(VecDeque::with_capacity(DEFAULT_RECENT_EVENT_LIMIT)),
                recent_limit: DEFAULT_RECENT_EVENT_LIMIT,
                slow_event_threshold_ms: DEFAULT_SLOW_EVENT_THRESHOLD_MS,
                writer,
            }),
        }
    }

    pub fn path(&self) -> &Path {
        &self.inner.path
    }

    pub fn record_duration(
        &self,
        operation: impl Into<String>,
        started_at: Instant,
        status: impl Into<String>,
        fields: RuntimeTelemetryFields,
        metrics: RuntimeTelemetryMetrics,
        error: Option<String>,
    ) {
        let elapsed_ms = started_at.elapsed().as_millis().min(u64::MAX as u128) as u64;
        self.record_event(RuntimeTelemetryEvent {
            recorded_at: now_iso(),
            operation: operation.into(),
            status: status.into(),
            elapsed_ms,
            slow: elapsed_ms >= self.inner.slow_event_threshold_ms,
            fields,
            metrics,
            error: error.map(|value| truncate_error(&value)),
        });
    }

    pub fn snapshot(&self, limit: usize) -> RuntimeTelemetrySnapshot {
        let guard = self
            .inner
            .recent
            .lock()
            .expect("runtime telemetry mutex poisoned");
        let retained_event_count = guard.len();
        let clamped_limit = limit.max(1).min(self.inner.recent_limit);
        let mut events = guard
            .iter()
            .rev()
            .take(clamped_limit)
            .cloned()
            .collect::<Vec<_>>();
        events.reverse();
        drop(guard);

        let summary = summarize_events(&events);
        RuntimeTelemetrySnapshot {
            path: self.inner.path.display().to_string(),
            retained_event_count,
            returned_event_count: events.len(),
            slow_event_threshold_ms: self.inner.slow_event_threshold_ms,
            events,
            summary,
        }
    }

    fn record_event(&self, event: RuntimeTelemetryEvent) {
        {
            let mut recent = self
                .inner
                .recent
                .lock()
                .expect("runtime telemetry mutex poisoned");
            if recent.len() >= self.inner.recent_limit {
                recent.pop_front();
            }
            recent.push_back(event.clone());
        }
        let _ = self.inner.writer.send(event);
    }
}

async fn append_event(path: &Path, event: &RuntimeTelemetryEvent) -> Result<()> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await.with_context(|| {
            format!("failed to create telemetry directory {}", parent.display())
        })?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .await
        .with_context(|| format!("failed to open runtime telemetry log {}", path.display()))?;
    let mut line = serde_json::to_vec(event)?;
    line.push(b'\n');
    file.write_all(&line)
        .await
        .with_context(|| format!("failed to append runtime telemetry log {}", path.display()))?;
    Ok(())
}

fn summarize_events(events: &[RuntimeTelemetryEvent]) -> Vec<RuntimeTelemetrySummaryItem> {
    let mut aggregates = BTreeMap::<String, RuntimeTelemetrySummaryAccumulator>::new();
    for event in events {
        let entry = aggregates.entry(event.operation.clone()).or_default();
        entry.count += 1;
        if event.slow {
            entry.slow_count += 1;
        }
        if event.error.is_some() || event.status == "error" {
            entry.error_count += 1;
        }
        entry.total_elapsed_ms = entry.total_elapsed_ms.saturating_add(event.elapsed_ms);
        entry.max_elapsed_ms = entry.max_elapsed_ms.max(event.elapsed_ms);
        entry.last_elapsed_ms = event.elapsed_ms;
    }

    let mut summary = aggregates
        .into_iter()
        .map(|(operation, item)| RuntimeTelemetrySummaryItem {
            operation,
            count: item.count,
            slow_count: item.slow_count,
            error_count: item.error_count,
            total_elapsed_ms: item.total_elapsed_ms,
            average_elapsed_ms: if item.count == 0 {
                0
            } else {
                item.total_elapsed_ms / item.count as u64
            },
            max_elapsed_ms: item.max_elapsed_ms,
            last_elapsed_ms: item.last_elapsed_ms,
        })
        .collect::<Vec<_>>();
    summary.sort_by(|left, right| {
        right
            .total_elapsed_ms
            .cmp(&left.total_elapsed_ms)
            .then_with(|| right.max_elapsed_ms.cmp(&left.max_elapsed_ms))
            .then_with(|| left.operation.cmp(&right.operation))
    });
    summary
}

fn truncate_error(value: &str) -> String {
    const LIMIT: usize = 240;
    if value.chars().count() <= LIMIT {
        return value.to_owned();
    }
    let truncated = value.chars().take(LIMIT).collect::<String>();
    format!("{truncated}...")
}

fn now_iso() -> String {
    Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    fn temp_path(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time before unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "threadbridge-runtime-telemetry-{name}-{unique}.jsonl"
        ))
    }

    #[tokio::test]
    async fn runtime_telemetry_summary_orders_by_total_elapsed() {
        let telemetry = RuntimeTelemetryHandle::new(temp_path("summary"));

        let fast_started = Instant::now() - Duration::from_millis(15);
        telemetry.record_duration(
            "workspace.ensure_runtime",
            fast_started,
            "ok",
            RuntimeTelemetryFields::new(),
            RuntimeTelemetryMetrics::new(),
            None,
        );

        let slow_started = Instant::now() - Duration::from_millis(80);
        telemetry.record_duration(
            "management.workspace_views",
            slow_started,
            "ok",
            RuntimeTelemetryFields::new(),
            RuntimeTelemetryMetrics::new(),
            None,
        );

        let snapshot = telemetry.snapshot(20);
        assert_eq!(snapshot.events.len(), 2);
        assert_eq!(
            snapshot.summary.first().map(|item| item.operation.as_str()),
            Some("management.workspace_views")
        );
        assert!(
            snapshot
                .summary
                .first()
                .is_some_and(|item| item.total_elapsed_ms >= 80)
        );
    }

    #[tokio::test]
    async fn runtime_telemetry_snapshot_limit_returns_latest_events() {
        let telemetry = RuntimeTelemetryHandle::new(temp_path("limit"));

        for index in 0..3 {
            let mut fields = RuntimeTelemetryFields::new();
            fields.insert("index".to_owned(), index.to_string());
            telemetry.record_duration(
                "desktop.collect_snapshot",
                Instant::now(),
                "ok",
                fields,
                RuntimeTelemetryMetrics::new(),
                None,
            );
        }

        let snapshot = telemetry.snapshot(2);
        assert_eq!(snapshot.events.len(), 2);
        assert_eq!(
            snapshot.events[0].fields.get("index").map(String::as_str),
            Some("1")
        );
        assert_eq!(
            snapshot.events[1].fields.get("index").map(String::as_str),
            Some("2")
        );
    }
}
