# Sourced by tmux-agent.sh. Preserve the v1 Claude session contract.
ADAPTER_CLI=claude
ADAPTER_PREFIX=tc
ADAPTER_METADATA=tmux_claude
ADAPTER_PERMISSION_MODE=bypassPermissions

adapter_prepare_start() {
  AGENT_SESSION_ID=$(new_session_uuid)
  ADAPTER_ARGV=(claude --permission-mode bypassPermissions --session-id "$AGENT_SESSION_ID")
}

adapter_validate_session() {
  local session=$1 value
  value=$(session_option "$session" session_id)
  if ! is_uuid "$value"; then
    printf 'refusing to use session %s: Claude session id missing or invalid (%s)\n' \
      "$session" "${value:-unset}" >&2
    exit 1
  fi
}

adapter_is_process() {
  [[ "$1" == claude ]]
}
