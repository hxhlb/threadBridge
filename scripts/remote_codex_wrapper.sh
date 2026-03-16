#!/usr/bin/env bash
set -euo pipefail

REAL_CODEX=${CODEX_REAL_BIN:-"$HOME/.local/lib/node-v22.22.1-darwin-x64/bin/codex"}

if [[ ! -x "$REAL_CODEX" ]]; then
  printf 'Missing real codex binary: %s\n' "$REAL_CODEX" >&2
  exit 1
fi

if [[ $# -gt 0 && "$1" == "exec" ]]; then
  shift
  filtered=()
  skip_next=0
  for arg in "$@"; do
    if [[ "$skip_next" == "1" ]]; then
      skip_next=0
      continue
    fi
    case "$arg" in
      --full-auto)
        continue
        ;;
      --sandbox|-s)
        skip_next=1
        continue
        ;;
    esac
    filtered+=("$arg")
  done

  exec "$REAL_CODEX" exec \
    --dangerously-bypass-approvals-and-sandbox \
    -c shell_environment_policy.inherit=all \
    "${filtered[@]}"
fi

exec "$REAL_CODEX" "$@"
