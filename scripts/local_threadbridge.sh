#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)
REPO_ROOT=$(cd "$SCRIPT_DIR/.." && pwd -P)
ENV_FILE="$REPO_ROOT/.env.local"
LOG_DIR="$REPO_ROOT/logs"
EVENT_LOG="$REPO_ROOT/data/debug/events.jsonl"
CARGO_HOME_DIR="${CARGO_HOME:-$REPO_ROOT/.cargo}"
CARGO_TARGET_DIR_PATH="${CARGO_TARGET_DIR:-$REPO_ROOT/target}"
BUILD_PROFILE="${BUILD_PROFILE:-dev}"
RUSTUP_HOME_DIR="${RUSTUP_HOME:-$HOME/.rustup}"
RUNTIME_PATH="$HOME/.cargo/bin:$REPO_ROOT/bin:$PATH"
MANAGED_CODEX_DIR="$REPO_ROOT/.threadbridge/codex"
MANAGED_CODEX_BIN="$MANAGED_CODEX_DIR/codex"
MANAGED_CODEX_SOURCE_FILE="$MANAGED_CODEX_DIR/source.txt"
MANAGED_CODEX_BUILD_INFO_FILE="$MANAGED_CODEX_DIR/build-info.txt"
CODEX_SOURCE_REPO="${CODEX_SOURCE_REPO:-/Volumes/Data/Github/codex}"
CODEX_SOURCE_RS_DIR="${CODEX_SOURCE_RS_DIR:-$CODEX_SOURCE_REPO/codex-rs}"
CODEX_BUILD_PROFILE="${CODEX_BUILD_PROFILE:-$BUILD_PROFILE}"
CODEX_CARGO_HOME_DIR="${CODEX_CARGO_HOME:-$HOME/.cargo}"
CODEX_CARGO_TARGET_DIR_PATH="${CODEX_CARGO_TARGET_DIR:-$CODEX_SOURCE_RS_DIR/target}"
CODEX_RUSTUP_HOME_DIR="${CODEX_RUSTUP_HOME:-$RUSTUP_HOME_DIR}"

usage() {
  cat <<'EOF'
Usage: local_threadbridge.sh <command> [--runtime headless|desktop] [--codex-source brew|source]

Commands:
  build
  start
  stop
  restart
  status
  logs

Options:
  --runtime headless|desktop   Choose which runtime binary to manage. Default: headless
  --codex-source brew|source  Choose which local codex binary hcodex should prefer.
                              The choice is persisted in .threadbridge/codex/source.txt.

Environment overrides:
  BUILD_PROFILE=dev|release      Build profile to run threadBridge. Default: dev
  CODEX_BUILD_PROFILE=dev|release
                                 Build profile for source-built Codex. Default: BUILD_PROFILE
  CODEX_SOURCE_REPO=/abs/path    Codex repo root. Default: /Volumes/Data/Github/codex
  CODEX_SOURCE_RS_DIR=/abs/path  Codex Rust workspace. Default: $CODEX_SOURCE_REPO/codex-rs
EOF
}

log() {
  printf '[local-threadbridge] %s\n' "$*"
}

read_codex_source_preference() {
  if [[ -f "$MANAGED_CODEX_SOURCE_FILE" ]]; then
    tr -d '\n' < "$MANAGED_CODEX_SOURCE_FILE"
    return 0
  fi
  printf '%s\n' 'brew'
}

write_codex_source_preference() {
  local source=$1
  mkdir -p "$MANAGED_CODEX_DIR"
  printf '%s\n' "$source" > "$MANAGED_CODEX_SOURCE_FILE"
}

resolve_codex_source() {
  local requested=${1:-}
  if [[ -n "$requested" ]]; then
    case "$requested" in
      brew|source)
        printf '%s\n' "$requested"
        return 0
        ;;
      alpha)
        log "codex source 'alpha' is deprecated; using 'source' instead"
        printf '%s\n' 'source'
        return 0
        ;;
      *)
        printf 'Unsupported codex source: %s\n' "$requested" >&2
        exit 1
        ;;
    esac
  fi

  local persisted
  persisted=$(read_codex_source_preference)
  case "$persisted" in
    brew|source)
      printf '%s\n' "$persisted"
      ;;
    alpha)
      printf '%s\n' 'source'
      ;;
    *)
      printf '%s\n' 'brew'
      ;;
  esac
}

ensure_source_codex_binary() {
  require_command cargo

  if [[ ! -d "$CODEX_SOURCE_RS_DIR" ]]; then
    printf 'Missing Codex source workspace: %s\n' "$CODEX_SOURCE_RS_DIR" >&2
    exit 1
  fi

  if [[ ! -f "$CODEX_SOURCE_RS_DIR/Cargo.toml" ]]; then
    printf 'Missing Codex Cargo.toml: %s\n' "$CODEX_SOURCE_RS_DIR/Cargo.toml" >&2
    exit 1
  fi

  mkdir -p "$MANAGED_CODEX_DIR"

  local source_binary profile_flag build_info git_rev
  case "$CODEX_BUILD_PROFILE" in
    dev)
      profile_flag=""
      source_binary="$CODEX_CARGO_TARGET_DIR_PATH/debug/codex"
      ;;
    release)
      profile_flag="--release"
      source_binary="$CODEX_CARGO_TARGET_DIR_PATH/release/codex"
      ;;
    *)
      printf 'Unsupported CODEX_BUILD_PROFILE: %s\n' "$CODEX_BUILD_PROFILE" >&2
      exit 1
      ;;
  esac

  log "building Codex from source ($CODEX_BUILD_PROFILE): $CODEX_SOURCE_RS_DIR"
  (
    cd "$CODEX_SOURCE_RS_DIR"
    export CARGO_HOME="$CODEX_CARGO_HOME_DIR"
    export CARGO_TARGET_DIR="$CODEX_CARGO_TARGET_DIR_PATH"
    export RUSTUP_HOME="$CODEX_RUSTUP_HOME_DIR"
    if [[ -n "$profile_flag" ]]; then
      cargo build "$profile_flag" -p codex-cli
    else
      cargo build -p codex-cli
    fi
  )

  if [[ ! -x "$source_binary" ]]; then
    printf 'Expected built Codex binary at %s\n' "$source_binary" >&2
    exit 1
  fi

  install -m 755 "$source_binary" "$MANAGED_CODEX_BIN"
  git_rev=$(git -C "$CODEX_SOURCE_REPO" rev-parse --short HEAD 2>/dev/null || printf 'unknown')
  build_info=$(cat <<EOF
source_repo=$CODEX_SOURCE_REPO
source_rs_dir=$CODEX_SOURCE_RS_DIR
build_profile=$CODEX_BUILD_PROFILE
git_rev=$git_rev
binary=$source_binary
EOF
)
  printf '%s\n' "$build_info" > "$MANAGED_CODEX_BUILD_INFO_FILE"
  log "source-built Codex binary ready: $MANAGED_CODEX_BIN"
}

require_command() {
  if ! command -v "$1" >/dev/null 2>&1; then
    printf 'Missing required command: %s\n' "$1" >&2
    exit 1
  fi
}

profile_output_dir() {
  case "$BUILD_PROFILE" in
    dev)
      printf '%s\n' "$CARGO_TARGET_DIR_PATH/debug"
      ;;
    release)
      printf '%s\n' "$CARGO_TARGET_DIR_PATH/release"
      ;;
    *)
      printf 'Unsupported BUILD_PROFILE: %s\n' "$BUILD_PROFILE" >&2
      exit 1
      ;;
  esac
}

binary_path() {
  local bin_name=${1:?missing bin name}
  printf '%s/%s\n' "$(profile_output_dir)" "$bin_name"
}

should_build_desktop() {
  [[ "$(uname -s)" == "Darwin" ]]
}

validate_runtime_mode() {
  local runtime_mode=${1:-headless}
  case "$runtime_mode" in
    headless)
      printf '%s\n' "$runtime_mode"
      ;;
    desktop)
      if ! should_build_desktop; then
        printf 'desktop runtime is only available on macOS\n' >&2
        exit 1
      fi
      printf '%s\n' "$runtime_mode"
      ;;
    *)
      printf 'Unsupported runtime mode: %s\n' "$runtime_mode" >&2
      exit 1
      ;;
  esac
}

other_runtime_mode() {
  local runtime_mode=${1:?missing runtime mode}
  case "$runtime_mode" in
    headless) printf '%s\n' desktop ;;
    desktop) printf '%s\n' headless ;;
    *) printf 'Unsupported runtime mode: %s\n' "$runtime_mode" >&2; exit 1 ;;
  esac
}

runtime_binary_name() {
  local runtime_mode=${1:?missing runtime mode}
  case "$runtime_mode" in
    headless) printf '%s\n' threadbridge ;;
    desktop) printf '%s\n' threadbridge_desktop ;;
    *) printf 'Unsupported runtime mode: %s\n' "$runtime_mode" >&2; exit 1 ;;
  esac
}

stdout_log_path() {
  local runtime_mode=${1:?missing runtime mode}
  printf '%s/local-threadbridge-%s.stdout.log\n' "$LOG_DIR" "$runtime_mode"
}

stderr_log_path() {
  local runtime_mode=${1:?missing runtime mode}
  printf '%s/local-threadbridge-%s.stderr.log\n' "$LOG_DIR" "$runtime_mode"
}

tmux_session_name() {
  local runtime_mode=${1:?missing runtime mode}
  local hash
  hash=$(printf '%s' "$REPO_ROOT" | shasum | awk '{print substr($1, 1, 10)}')
  printf 'threadbridge-%s-%s' "$hash" "$runtime_mode"
}

tmux_session_exists() {
  local session_name=$1
  tmux has-session -t "$session_name" 2>/dev/null
}

tmux_session_pid() {
  local session_name=$1
  tmux list-panes -t "$session_name" -F '#{pane_pid}' 2>/dev/null | head -n 1
}

ensure_layout() {
  mkdir -p "$LOG_DIR" "$REPO_ROOT/data/debug" "$MANAGED_CODEX_DIR"
  touch \
    "$(stdout_log_path headless)" \
    "$(stderr_log_path headless)" \
    "$EVENT_LOG"
  if should_build_desktop; then
    touch \
      "$(stdout_log_path desktop)" \
      "$(stderr_log_path desktop)"
  fi
}

ensure_env_file() {
  [[ -f "$ENV_FILE" ]]
}

ensure_env() {
  if ! ensure_env_file; then
    printf 'Missing env file: %s\n' "$ENV_FILE" >&2
    exit 1
  fi
  local token
  token=$(sed -n 's/^TELEGRAM_BOT_TOKEN=//p' "$ENV_FILE" | head -n 1)
  if [[ -z "$token" ]]; then
    printf 'Set TELEGRAM_BOT_TOKEN in %s before starting.\n' "$ENV_FILE" >&2
    exit 1
  fi
}

build_runtime_binaries() {
  local build_args=(build)
  if [[ "$BUILD_PROFILE" == "release" ]]; then
    build_args+=(--release)
  fi
  build_args+=(--bin threadbridge)
  if should_build_desktop; then
    build_args+=(--bin threadbridge_desktop)
  fi

  log "building threadbridge runtime binaries ($BUILD_PROFILE)"
  (
    cd "$REPO_ROOT"
    export PATH="$RUNTIME_PATH"
    export CARGO_HOME="$CARGO_HOME_DIR"
    export CARGO_TARGET_DIR="$CARGO_TARGET_DIR_PATH"
    export RUSTUP_HOME="$RUSTUP_HOME_DIR"
    cargo "${build_args[@]}"
  )

  log "built binary: $(binary_path threadbridge)"
  if should_build_desktop; then
    log "built binary: $(binary_path threadbridge_desktop)"
  fi
}

build_local() {
  local codex_source=${1:-}
  ensure_layout
  require_command cargo

  codex_source=$(resolve_codex_source "$codex_source")
  write_codex_source_preference "$codex_source"

  if [[ "$codex_source" == "source" ]]; then
    ensure_source_codex_binary
  else
    log "using brew/system codex as primary local CLI source"
  fi

  build_runtime_binaries
}

stop_conflicting_runtime() {
  local runtime_mode=${1:?missing runtime mode}
  local other_mode
  other_mode=$(other_runtime_mode "$runtime_mode")
  if [[ "$other_mode" == "desktop" ]] && ! should_build_desktop; then
    return 0
  fi
  local other_session
  other_session=$(tmux_session_name "$other_mode")
  if tmux_session_exists "$other_session"; then
    log "stopping conflicting $other_mode runtime session: $other_session"
    tmux kill-session -t "$other_session"
    sleep 1
  fi
}

start_runtime() {
  local runtime_mode=${1:-headless}
  local codex_source=${2:-}
  ensure_layout
  require_command cargo
  require_command tmux
  runtime_mode=$(validate_runtime_mode "$runtime_mode")
  if [[ "$runtime_mode" == "headless" ]]; then
    ensure_env
  fi

  codex_source=$(resolve_codex_source "$codex_source")
  write_codex_source_preference "$codex_source"

  if [[ "$codex_source" == "source" ]]; then
    ensure_source_codex_binary
  else
    log "using brew/system codex as primary local CLI source"
  fi
  build_runtime_binaries

  local runtime_binary_name_value runtime_binary stdout_log stderr_log
  runtime_binary_name_value=$(runtime_binary_name "$runtime_mode")
  runtime_binary=$(binary_path "$runtime_binary_name_value")
  stdout_log=$(stdout_log_path "$runtime_mode")
  stderr_log=$(stderr_log_path "$runtime_mode")
  if [[ ! -x "$runtime_binary" ]]; then
    printf 'Missing built binary: %s\n' "$runtime_binary" >&2
    exit 1
  fi

  local session_name
  session_name=$(tmux_session_name "$runtime_mode")
  stop_conflicting_runtime "$runtime_mode"
  if tmux_session_exists "$session_name"; then
    log "stopping existing tmux session: $session_name"
    tmux kill-session -t "$session_name"
    sleep 1
  fi

  local launch_command
  launch_command=$(printf 'cd %q && export PATH=%q CARGO_HOME=%q CARGO_TARGET_DIR=%q RUSTUP_HOME=%q && if [[ -f %q ]]; then set -a && source %q && set +a; fi && exec %q >>%q 2>>%q' \
    "$REPO_ROOT" \
    "$RUNTIME_PATH" \
    "$CARGO_HOME_DIR" \
    "$CARGO_TARGET_DIR_PATH" \
    "$RUSTUP_HOME_DIR" \
    "$ENV_FILE" \
    "$ENV_FILE" \
    "$runtime_binary" \
    "$stdout_log" \
    "$stderr_log")
  tmux new-session -d -s "$session_name" "$(printf 'bash -lc %q' "$launch_command")"

  sleep 3
  if ! tmux_session_exists "$session_name"; then
    log "threadbridge failed to start"
    tail -n 80 "$STDERR_LOG" || true
    exit 1
  fi

  log "$runtime_mode runtime started in tmux session: $session_name"
  log "codex source preference: $codex_source"
  status_runtime "$runtime_mode"
}

stop_runtime() {
  local runtime_mode=${1:-headless}
  runtime_mode=$(validate_runtime_mode "$runtime_mode")
  local session_name
  session_name=$(tmux_session_name "$runtime_mode")

  if ! tmux_session_exists "$session_name"; then
    log "$runtime_mode runtime is not running"
    return 0
  fi

  tmux kill-session -t "$session_name"
  log "$runtime_mode runtime stopped"
}

status_runtime() {
  local runtime_mode=${1:-headless}
  runtime_mode=$(validate_runtime_mode "$runtime_mode")
  local session_name
  session_name=$(tmux_session_name "$runtime_mode")
  local codex_source
  codex_source=$(resolve_codex_source "")

  if ! tmux_session_exists "$session_name"; then
    log "$runtime_mode runtime is not running"
  else
    local pane_pid
    pane_pid=$(tmux_session_pid "$session_name")
    log "$runtime_mode runtime running in tmux session: $session_name"
    if [[ -n "$pane_pid" ]]; then
      log "tmux pane PID: $pane_pid"
    fi
  fi
  log "codex source preference: $codex_source"
  if [[ "$codex_source" == "source" && -f "$MANAGED_CODEX_BUILD_INFO_FILE" ]]; then
    while IFS= read -r line; do
      [[ -n "$line" ]] && log "managed Codex $line"
    done < "$MANAGED_CODEX_BUILD_INFO_FILE"
  fi

  if [[ -f "$EVENT_LOG" ]]; then
    log "recent events"
    tail -n 20 "$EVENT_LOG" || true
  fi
}

logs_runtime() {
  local runtime_mode=${1:-headless}
  runtime_mode=$(validate_runtime_mode "$runtime_mode")
  ensure_layout
  local session_name
  session_name=$(tmux_session_name "$runtime_mode")
  local stdout_log stderr_log
  stdout_log=$(stdout_log_path "$runtime_mode")
  stderr_log=$(stderr_log_path "$runtime_mode")

  if tmux_session_exists "$session_name"; then
    log "tmux pane"
    tmux capture-pane -p -t "$session_name" -S -40 || true
  fi

  log "stdout"
  tail -n 40 "$stdout_log" || true
  log "stderr"
  tail -n 40 "$stderr_log" || true
  log "events"
  tail -n 40 "$EVENT_LOG" || true
}

main() {
  local command=${1:-}
  local codex_source=""
  local runtime_mode="headless"
  shift || true
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --runtime)
        shift
        if [[ $# -eq 0 ]]; then
          printf 'Missing value for --runtime\n' >&2
          exit 1
        fi
        runtime_mode=$1
        ;;
      --codex-source)
        shift
        if [[ $# -eq 0 ]]; then
          printf 'Missing value for --codex-source\n' >&2
          exit 1
        fi
        codex_source=$1
        ;;
      *)
        printf 'Unknown argument: %s\n' "$1" >&2
        usage
        exit 1
        ;;
    esac
    shift
  done
  case "$command" in
    build)
      build_local "$codex_source"
      ;;
    start)
      start_runtime "$runtime_mode" "$codex_source"
      ;;
    stop)
      stop_runtime "$runtime_mode"
      ;;
    restart)
      stop_runtime "$runtime_mode"
      start_runtime "$runtime_mode" "$codex_source"
      ;;
    status)
      status_runtime "$runtime_mode"
      ;;
    logs)
      logs_runtime "$runtime_mode"
      ;;
    *)
      usage
      exit 1
      ;;
  esac
}

main "$@"
