#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)
REPO_ROOT=$(cd "$SCRIPT_DIR/.." && pwd -P)
ENV_FILE="$REPO_ROOT/.env.local"
LOG_DIR="$REPO_ROOT/logs"
STDOUT_LOG="$LOG_DIR/local-artbot.stdout.log"
STDERR_LOG="$LOG_DIR/local-artbot.stderr.log"
EVENT_LOG="$REPO_ROOT/data/debug/events.jsonl"
CARGO_HOME_DIR="${CARGO_HOME:-$REPO_ROOT/.cargo}"
CARGO_TARGET_DIR_PATH="${CARGO_TARGET_DIR:-$REPO_ROOT/target}"
BUILD_PROFILE="${BUILD_PROFILE:-dev}"
RUSTUP_HOME_DIR="${RUSTUP_HOME:-$HOME/.rustup}"
RUNTIME_PATH="$HOME/.cargo/bin:$REPO_ROOT/bin:$PATH"

usage() {
  cat <<'EOF'
Usage: local_artbot.sh <command>

Commands:
  start
  stop
  restart
  status
  logs

Environment overrides:
  BUILD_PROFILE=dev|release   Build profile to run. Default: dev
EOF
}

log() {
  printf '[local-artbot] %s\n' "$*"
}

require_command() {
  if ! command -v "$1" >/dev/null 2>&1; then
    printf 'Missing required command: %s\n' "$1" >&2
    exit 1
  fi
}

process_cwd() {
  local pid=$1
  local cwd
  cwd=$(lsof -a -p "$pid" -d cwd 2>/dev/null | awk 'NR==2 {print $NF}')
  if [[ -n "$cwd" && -d "$cwd" ]]; then
    (cd "$cwd" && pwd -P)
  else
    printf '%s\n' "$cwd"
  fi
}

binary_path() {
  case "$BUILD_PROFILE" in
    dev)
      printf '%s\n' "$CARGO_TARGET_DIR_PATH/debug/artbot"
      ;;
    release)
      printf '%s\n' "$CARGO_TARGET_DIR_PATH/release/artbot"
      ;;
    *)
      printf 'Unsupported BUILD_PROFILE: %s\n' "$BUILD_PROFILE" >&2
      exit 1
      ;;
  esac
}

bot_pids() {
  {
    pgrep -x artbot || true
    pgrep -f 'cargo run --bin artbot' || true
  } | awk 'NF { print $1 }' | sort -u | while IFS= read -r pid; do
    [[ -n "$pid" ]] || continue
    if [[ "$(process_cwd "$pid")" == "$REPO_ROOT" ]]; then
      printf '%s\n' "$pid"
    fi
  done
}

kill_bot_processes() {
  local pids
  pids=$(bot_pids)
  if [[ -z "$pids" ]]; then
    return 0
  fi

  log "stopping existing bot process(es): $(echo "$pids" | tr '\n' ' ')"
  while IFS= read -r pid; do
    [[ -n "$pid" ]] || continue
    kill "$pid" || true
  done <<< "$pids"

  sleep 2

  local remaining
  remaining=$(bot_pids)
  if [[ -n "$remaining" ]]; then
    log "force killing lingering bot process(es): $(echo "$remaining" | tr '\n' ' ')"
    while IFS= read -r pid; do
      [[ -n "$pid" ]] || continue
      kill -9 "$pid" || true
    done <<< "$remaining"
    sleep 1
  fi
}

ensure_layout() {
  mkdir -p "$LOG_DIR" "$REPO_ROOT/data/debug"
  touch "$STDOUT_LOG" "$STDERR_LOG" "$EVENT_LOG"
}

ensure_env() {
  if [[ ! -f "$ENV_FILE" ]]; then
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

build_bot() {
  log "building artbot ($BUILD_PROFILE)"
  (
    cd "$REPO_ROOT"
    export PATH="$RUNTIME_PATH"
    export CARGO_HOME="$CARGO_HOME_DIR"
    export CARGO_TARGET_DIR="$CARGO_TARGET_DIR_PATH"
    export RUSTUP_HOME="$RUSTUP_HOME_DIR"
    if [[ "$BUILD_PROFILE" == "release" ]]; then
      cargo build --release --bin artbot
    else
      cargo build --bin artbot
    fi
  )
}

start_bot() {
  ensure_layout
  ensure_env
  require_command cargo
  require_command pgrep
  require_command lsof

  kill_bot_processes

  build_bot

  local bot_binary
  bot_binary=$(binary_path)
  if [[ ! -x "$bot_binary" ]]; then
    printf 'Missing built binary: %s\n' "$bot_binary" >&2
    exit 1
  fi

  nohup env \
    PATH="$RUNTIME_PATH" \
    CARGO_HOME="$CARGO_HOME_DIR" \
    CARGO_TARGET_DIR="$CARGO_TARGET_DIR_PATH" \
    RUSTUP_HOME="$RUSTUP_HOME_DIR" \
    bash -lc "cd '$REPO_ROOT' && set -a && source '$ENV_FILE' && set +a && exec '$bot_binary'" \
    >"$STDOUT_LOG" 2>"$STDERR_LOG" < /dev/null &

  sleep 3
  if [[ -z "$(bot_pids)" ]]; then
    log "artbot failed to start"
    tail -n 80 "$STDERR_LOG" || true
    exit 1
  fi

  log "artbot started"
  status_bot
}

stop_bot() {
  if [[ -z "$(bot_pids)" ]]; then
    log "artbot is not running"
    return 0
  fi

  kill_bot_processes
  log "artbot stopped"
}

status_bot() {
  local pids
  pids=$(bot_pids)
  if [[ -z "$pids" ]]; then
    log "artbot is not running"
  else
    log "artbot running with PID(s): $(echo "$pids" | tr '\n' ' ')"
  fi

  if [[ -f "$EVENT_LOG" ]]; then
    log "recent events"
    tail -n 20 "$EVENT_LOG" || true
  fi
}

logs_bot() {
  ensure_layout
  log "stdout"
  tail -n 40 "$STDOUT_LOG" || true
  log "stderr"
  tail -n 40 "$STDERR_LOG" || true
  log "events"
  tail -n 40 "$EVENT_LOG" || true
}

main() {
  local command=${1:-}
  case "$command" in
    start)
      start_bot
      ;;
    stop)
      stop_bot
      ;;
    restart)
      stop_bot
      start_bot
      ;;
    status)
      status_bot
      ;;
    logs)
      logs_bot
      ;;
    *)
      usage
      exit 1
      ;;
  esac
}

main "$@"
