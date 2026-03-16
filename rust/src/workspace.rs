use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use tokio::fs;

pub const WORKSPACE_RUNTIME_CONTRACT_HEADING: &str = "## Workspace Runtime Contract";

fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn extract_markdown_section(markdown: &str, heading: &str) -> Option<String> {
    let start = markdown.find(heading)?;
    let next = markdown[start + heading.len()..]
        .find("\n## ")
        .map(|offset| start + heading.len() + offset + 1)
        .unwrap_or(markdown.len());
    Some(markdown[start..next].trim_end().to_owned())
}

fn sync_workspace_runtime_contract(markdown: &str, runtime_section: &str) -> String {
    if let Some(current) = extract_markdown_section(markdown, WORKSPACE_RUNTIME_CONTRACT_HEADING) {
        return format!(
            "{}\n",
            markdown
                .trim_end()
                .replacen(&current, runtime_section.trim_end(), 1)
                .trim_end()
        );
    }
    format!(
        "{}\n\n{}\n",
        markdown.trim_end(),
        runtime_section.trim_end()
    )
}

fn build_wrapper_script(tool_file_name: &str, repo_root: &Path) -> String {
    let quoted_repo_root = shell_single_quote(&repo_root.display().to_string());
    [
        "#!/bin/sh",
        "set -eu",
        "SCRIPT_DIR=\"$(CDPATH= cd -- \"$(dirname \"$0\")\" && pwd)\"",
        "WORKSPACE_DIR=\"$(CDPATH= cd -- \"$SCRIPT_DIR/..\" && pwd)\"",
        &format!("REPO_ROOT={quoted_repo_root}"),
        "cd \"$WORKSPACE_DIR\"",
        &format!(
            "exec python3 \"$REPO_ROOT/tools/{tool_file_name}\" --repo-root \"$REPO_ROOT\" \"$@\""
        ),
        "",
    ]
    .join("\n")
}

pub async fn ensure_workspace_runtime(
    repo_root: &Path,
    seed_template_path: &Path,
    workspace_path: &Path,
) -> Result<()> {
    fs::create_dir_all(workspace_path).await.with_context(|| {
        format!(
            "failed to create workspace directory: {}",
            workspace_path.display()
        )
    })?;

    let agents_path = workspace_path.join("AGENTS.md");
    let seed_agents_text = fs::read_to_string(seed_template_path)
        .await
        .with_context(|| {
            format!(
                "failed to read seed AGENTS.md: {}",
                seed_template_path.display()
            )
        })?;
    let runtime_contract =
        extract_markdown_section(&seed_agents_text, WORKSPACE_RUNTIME_CONTRACT_HEADING)
            .ok_or_else(|| {
                anyhow::anyhow!("Seed AGENTS.md is missing the workspace runtime contract section.")
            })?;

    match fs::read_to_string(&agents_path).await {
        Ok(existing) => {
            let synced = sync_workspace_runtime_contract(&existing, &runtime_contract);
            if synced != existing {
                fs::write(&agents_path, synced).await.with_context(|| {
                    format!("failed to write AGENTS.md: {}", agents_path.display())
                })?;
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::copy(seed_template_path, &agents_path)
                .await
                .with_context(|| format!("failed to seed AGENTS.md: {}", agents_path.display()))?;
        }
        Err(error) => {
            return Err(error).with_context(|| format!("failed to read {}", agents_path.display()));
        }
    }

    let bin_dir = workspace_path.join("bin");
    let tool_requests_dir = workspace_path.join("tool_requests");
    fs::create_dir_all(&bin_dir).await?;
    fs::create_dir_all(&tool_requests_dir).await?;

    for (tool, filename) in [
        ("build_prompt_config.py", "build_prompt_config"),
        ("generate_image.py", "generate_image"),
        ("send_telegram_media.py", "send_telegram_media"),
    ] {
        let wrapper_path = bin_dir.join(filename);
        fs::write(&wrapper_path, build_wrapper_script(tool, repo_root))
            .await
            .with_context(|| format!("failed to write wrapper: {}", wrapper_path.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let metadata = fs::metadata(&wrapper_path).await?;
            let mut permissions = metadata.permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&wrapper_path, permissions).await?;
        }
    }

    Ok(())
}

pub async fn ensure_linked_workspace_runtime(
    repo_root: &Path,
    seed_template_path: &Path,
    workspace_link_path: &Path,
    target_workspace_path: &Path,
) -> Result<()> {
    let parent = workspace_link_path.parent().ok_or_else(|| {
        anyhow::anyhow!(
            "workspace link path has no parent: {}",
            workspace_link_path.display()
        )
    })?;
    fs::create_dir_all(parent).await?;
    fs::create_dir_all(target_workspace_path)
        .await
        .with_context(|| format!("failed to create {}", target_workspace_path.display()))?;

    match fs::symlink_metadata(workspace_link_path).await {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() {
                let current = fs::read_link(workspace_link_path).await.with_context(|| {
                    format!("failed to read workspace symlink {}", workspace_link_path.display())
                })?;
                if current != target_workspace_path {
                    fs::remove_file(workspace_link_path).await?;
                }
            } else {
                bail!(
                    "workspace link path is not a symlink: {}",
                    workspace_link_path.display()
                );
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(error)
                .with_context(|| format!("failed to inspect {}", workspace_link_path.display()));
        }
    }

    if !fs::try_exists(workspace_link_path).await? {
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(target_workspace_path, workspace_link_path)
                .with_context(|| {
                    format!(
                        "failed to create workspace symlink {} -> {}",
                        workspace_link_path.display(),
                        target_workspace_path.display()
                    )
                })?;
        }
    }

    ensure_workspace_runtime(repo_root, seed_template_path, workspace_link_path).await
}

pub fn validate_seed_template(seed_template_path: &Path) -> Result<PathBuf> {
    if !seed_template_path.exists() {
        bail!(
            "Missing template AGENTS.md: {}",
            seed_template_path.display()
        );
    }
    Ok(seed_template_path.to_path_buf())
}
