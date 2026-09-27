#!/usr/bin/env bash
# Compatibility entry inside the renamed Plugin; all behavior uses the shared core.
set -euo pipefail
SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)
exec bash "$SCRIPT_DIR/tmux-agent.sh" claude "$@"
