# Sourced by tmux-agent.sh. Codex has no Claude-style creation --session-id.
ADAPTER_CLI=codex
ADAPTER_PREFIX=ta-codex
ADAPTER_METADATA=tmux_codex
ADAPTER_PERMISSION_MODE=workspace-write/never

adapter_prepare_start() {
  AGENT_SESSION_ID=""
  ADAPTER_ARGV=(codex --no-alt-screen --sandbox workspace-write --ask-for-approval never)
}

adapter_validate_session() {
  # A Codex native session UUID is not required or guessed from user history.
  :
}

adapter_is_process() {
  [[ "$1" == codex ]]
}
