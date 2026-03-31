use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use directories::BaseDirs;

const DEBUG_DATA_ROOT: &str = "./data";
const APP_DATA_DIR_NAME: &str = "threadBridge";
const DEBUG_EVENTS_RELATIVE_PATH: &str = "debug/events.jsonl";

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum BuildFlavor {
    Debug,
    Release,
}

impl BuildFlavor {
    pub fn current() -> Self {
        if cfg!(debug_assertions) {
            Self::Debug
        } else {
            Self::Release
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct RuntimePathOverrides {
    pub data_root: Option<String>,
    pub bot_data_path: Option<String>,
    pub debug_log_path: Option<String>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct RuntimePaths {
    pub data_root_path: PathBuf,
    pub debug_log_path: PathBuf,
}

pub fn resolve_runtime_paths(overrides: RuntimePathOverrides) -> Result<RuntimePaths> {
    let cwd = std::env::current_dir().context("failed to read current working directory")?;
    resolve_runtime_paths_with(
        &cwd,
        BuildFlavor::current(),
        default_local_data_dir(),
        overrides,
    )
}

fn default_local_data_dir() -> Option<PathBuf> {
    BaseDirs::new().map(|dirs| dirs.data_local_dir().to_path_buf())
}

fn resolve_runtime_paths_with(
    cwd: &Path,
    build_flavor: BuildFlavor,
    platform_local_data_dir: Option<PathBuf>,
    overrides: RuntimePathOverrides,
) -> Result<RuntimePaths> {
    let data_root_path = resolve_data_root(cwd, build_flavor, platform_local_data_dir, &overrides)?;
    let debug_log_path = match overrides.debug_log_path {
        Some(path) => resolve_from_base(cwd, path),
        None => data_root_path.join(DEBUG_EVENTS_RELATIVE_PATH),
    };
    Ok(RuntimePaths {
        data_root_path,
        debug_log_path,
    })
}

fn resolve_data_root(
    cwd: &Path,
    build_flavor: BuildFlavor,
    platform_local_data_dir: Option<PathBuf>,
    overrides: &RuntimePathOverrides,
) -> Result<PathBuf> {
    if let Some(path) = overrides.data_root.clone() {
        return Ok(resolve_from_base(cwd, path));
    }

    if let Some(path) = overrides.bot_data_path.clone() {
        let bot_data_path = resolve_from_base(cwd, path);
        return Ok(bot_data_path
            .parent()
            .map(PathBuf::from)
            .unwrap_or_else(|| cwd.join(DEBUG_DATA_ROOT)));
    }

    match build_flavor {
        BuildFlavor::Debug => Ok(resolve_from_base(cwd, DEBUG_DATA_ROOT)),
        BuildFlavor::Release => {
            let root = platform_local_data_dir.context(
                "failed to resolve a local application data directory for the release runtime",
            )?;
            Ok(root.join(APP_DATA_DIR_NAME))
        }
    }
}

fn resolve_from_base(base: &Path, input: impl AsRef<Path>) -> PathBuf {
    let path = input.as_ref();
    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    };
    joined
        .canonicalize()
        .unwrap_or_else(|_| joined.components().collect())
}

#[cfg(test)]
mod tests {
    use super::{
        BuildFlavor, DEBUG_EVENTS_RELATIVE_PATH, RuntimePathOverrides, resolve_runtime_paths_with,
    };
    use std::path::{Path, PathBuf};
    use uuid::Uuid;

    fn temp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "threadbridge-runtime-paths-{name}-{}",
            Uuid::new_v4()
        ))
    }

    fn path_string(path: &Path) -> String {
        path.display().to_string()
    }

    #[test]
    fn debug_build_defaults_to_repo_local_data_root() {
        let cwd = temp_path("debug-default");
        let paths = resolve_runtime_paths_with(
            &cwd,
            BuildFlavor::Debug,
            Some(temp_path("platform")),
            RuntimePathOverrides::default(),
        )
        .unwrap();
        assert_eq!(paths.data_root_path, cwd.join("data"));
        assert_eq!(
            paths.debug_log_path,
            cwd.join("data").join(DEBUG_EVENTS_RELATIVE_PATH)
        );
    }

    #[test]
    fn release_build_defaults_to_platform_local_data_dir() {
        let cwd = temp_path("release-default");
        let platform_root = temp_path("platform");
        let paths = resolve_runtime_paths_with(
            &cwd,
            BuildFlavor::Release,
            Some(platform_root.clone()),
            RuntimePathOverrides::default(),
        )
        .unwrap();
        assert_eq!(paths.data_root_path, platform_root.join("threadBridge"));
        assert_eq!(
            paths.debug_log_path,
            platform_root
                .join("threadBridge")
                .join(DEBUG_EVENTS_RELATIVE_PATH)
        );
    }

    #[test]
    fn data_root_override_has_highest_precedence() {
        let cwd = temp_path("override");
        let paths = resolve_runtime_paths_with(
            &cwd,
            BuildFlavor::Release,
            None,
            RuntimePathOverrides {
                data_root: Some("./custom-data".to_owned()),
                bot_data_path: Some("./ignored/state.json".to_owned()),
                debug_log_path: None,
            },
        )
        .unwrap();
        assert_eq!(paths.data_root_path, cwd.join("custom-data"));
        assert_eq!(
            paths.debug_log_path,
            cwd.join("custom-data").join(DEBUG_EVENTS_RELATIVE_PATH)
        );
    }

    #[test]
    fn legacy_bot_data_path_uses_parent_directory() {
        let cwd = temp_path("legacy");
        let paths = resolve_runtime_paths_with(
            &cwd,
            BuildFlavor::Release,
            Some(temp_path("platform")),
            RuntimePathOverrides {
                data_root: None,
                bot_data_path: Some("./legacy/state.json".to_owned()),
                debug_log_path: None,
            },
        )
        .unwrap();
        assert_eq!(paths.data_root_path, cwd.join("legacy"));
    }

    #[test]
    fn debug_log_override_is_resolved_relative_to_cwd() {
        let cwd = temp_path("debug-log");
        let paths = resolve_runtime_paths_with(
            &cwd,
            BuildFlavor::Debug,
            None,
            RuntimePathOverrides {
                data_root: Some("./custom-data".to_owned()),
                bot_data_path: None,
                debug_log_path: Some("./logs/custom.jsonl".to_owned()),
            },
        )
        .unwrap();
        assert_eq!(paths.data_root_path, cwd.join("custom-data"));
        assert_eq!(paths.debug_log_path, cwd.join("logs/custom.jsonl"));
    }

    #[test]
    fn release_build_requires_platform_local_data_dir_without_overrides() {
        let error = resolve_runtime_paths_with(
            &temp_path("missing-platform"),
            BuildFlavor::Release,
            None,
            RuntimePathOverrides::default(),
        )
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("failed to resolve a local application data directory")
        );
    }

    #[test]
    fn absolute_overrides_are_preserved() {
        let cwd = temp_path("absolute");
        let data_root = temp_path("explicit-data-root");
        let debug_log = temp_path("explicit-debug-log").join("events.jsonl");
        let paths = resolve_runtime_paths_with(
            &cwd,
            BuildFlavor::Debug,
            None,
            RuntimePathOverrides {
                data_root: Some(path_string(&data_root)),
                bot_data_path: None,
                debug_log_path: Some(path_string(&debug_log)),
            },
        )
        .unwrap();
        assert_eq!(paths.data_root_path, data_root);
        assert_eq!(paths.debug_log_path, debug_log);
    }
}
