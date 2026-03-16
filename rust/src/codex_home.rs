use std::collections::HashMap;
use std::env;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;
use walkdir::WalkDir;

#[derive(Debug, Clone)]
pub struct CodexHome {
    root: PathBuf,
}

#[derive(Debug, Clone)]
pub struct CodexSessionSummary {
    pub id: String,
    pub title: String,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct CodexSessionRecord {
    pub id: String,
    pub title: String,
    pub updated_at: Option<String>,
    pub cwd: PathBuf,
    pub rollout_path: PathBuf,
}

#[derive(Debug, Deserialize)]
struct SessionIndexEntry {
    id: String,
    #[serde(default)]
    thread_name: String,
    updated_at: String,
}

#[derive(Debug, Deserialize)]
struct SessionMetaEnvelope {
    #[serde(rename = "type")]
    kind: String,
    payload: SessionMetaPayload,
}

#[derive(Debug, Deserialize)]
struct SessionMetaPayload {
    id: String,
    cwd: String,
}

impl CodexHome {
    pub fn discover() -> Result<Self> {
        let home = env::var_os("HOME").context("HOME is not set")?;
        let root = PathBuf::from(home).join(".codex");
        if !root.exists() {
            anyhow::bail!("Missing Codex home: {}", root.display());
        }
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn list_recent_sessions(&self, limit: usize) -> Result<Vec<CodexSessionSummary>> {
        let mut sessions = self
            .read_session_index()?
            .into_values()
            .map(|entry| CodexSessionSummary {
                id: entry.id,
                title: if entry.thread_name.trim().is_empty() {
                    "(untitled session)".to_owned()
                } else {
                    entry.thread_name
                },
                updated_at: entry.updated_at,
            })
            .collect::<Vec<_>>();
        sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        sessions.truncate(limit);
        Ok(sessions)
    }

    pub fn resolve_session(&self, session_id: &str) -> Result<Option<CodexSessionRecord>> {
        let index = self.read_session_index()?;
        let rollout_path = match self.find_rollout_path(session_id)? {
            Some(path) => path,
            None => return Ok(None),
        };
        let meta = self.read_session_meta(&rollout_path)?;
        let index_entry = index.get(session_id);
        Ok(Some(CodexSessionRecord {
            id: meta.id,
            title: index_entry
                .map(|entry| entry.thread_name.clone())
                .filter(|title| !title.trim().is_empty())
                .unwrap_or_else(|| "(untitled session)".to_owned()),
            updated_at: index_entry.map(|entry| entry.updated_at.clone()),
            cwd: PathBuf::from(meta.cwd),
            rollout_path,
        }))
    }

    fn read_session_index(&self) -> Result<HashMap<String, SessionIndexEntry>> {
        let path = self.root.join("session_index.jsonl");
        if !path.exists() {
            return Ok(HashMap::new());
        }
        let file =
            File::open(&path).with_context(|| format!("failed to open {}", path.display()))?;
        let reader = BufReader::new(file);
        let mut entries = HashMap::new();
        for line in reader.lines() {
            let line = line?;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let entry: SessionIndexEntry = serde_json::from_str(trimmed)
                .with_context(|| format!("invalid session index line in {}", path.display()))?;
            entries.insert(entry.id.clone(), entry);
        }
        Ok(entries)
    }

    fn find_rollout_path(&self, session_id: &str) -> Result<Option<PathBuf>> {
        let sessions_root = self.root.join("sessions");
        if !sessions_root.exists() {
            return Ok(None);
        }
        for entry in WalkDir::new(&sessions_root)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if !entry.file_type().is_file() {
                continue;
            }
            let name = entry.file_name().to_string_lossy();
            if name.ends_with(".jsonl") && name.contains(session_id) {
                return Ok(Some(entry.into_path()));
            }
        }
        Ok(None)
    }

    fn read_session_meta(&self, rollout_path: &Path) -> Result<SessionMetaPayload> {
        let file = File::open(rollout_path)
            .with_context(|| format!("failed to open {}", rollout_path.display()))?;
        let reader = BufReader::new(file);
        for line in reader.lines() {
            let line = line?;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let envelope: SessionMetaEnvelope =
                serde_json::from_str(trimmed).with_context(|| {
                    format!("invalid session metadata in {}", rollout_path.display())
                })?;
            if envelope.kind == "session_meta" {
                return Ok(envelope.payload);
            }
        }
        anyhow::bail!(
            "missing session_meta event in session rollout {}",
            rollout_path.display()
        );
    }
}
