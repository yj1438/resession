#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)
AGENT=${1:-}
if [[ -z "$AGENT" || "$AGENT" == --help || "$AGENT" == -h ]]; then
  printf 'Usage: tmux-agent.sh <agent> <command> [args...]\nAdapters:\n'
  for adapter in "$SCRIPT_DIR"/adapters/*.sh; do
    basename "$adapter" .sh
  done
  exit 0
fi
# Only load shipped adapters, never an arbitrary path or shell expression.
if [[ ! "$AGENT" =~ ^[a-z][a-z0-9-]*$ || ! -f "$SCRIPT_DIR/adapters/$AGENT.sh" ]]; then
  printf 'unsupported agent: %s\n' "$AGENT" >&2
  exit 1
fi
shift
source "$SCRIPT_DIR/adapters/$AGENT.sh"
source "$SCRIPT_DIR/lib/core.sh"
main "$@"
