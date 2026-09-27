#!/usr/bin/env python3
"""Real tmux transport tests with inert CLI shims, no model or user sessions."""
import os
from pathlib import Path
import shlex
import shutil
import subprocess
import tempfile
import time
import unittest

SCRIPTS = Path(__file__).resolve().parents[1] / 'skills/scripts'
TMUX = shutil.which('tmux')
@unittest.skipUnless(TMUX, 'requires tmux')
class BridgeTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix='tma-', dir='/tmp')
        self.base = Path(self.tmp.name)
        self.project = self.base / 'project with space'
        self.project.mkdir()
        subprocess.run(['git', 'init', '-q', '-b', 'feature/test', str(self.project)], check=True)
        self.bin = self.base / 'bin'
        self.bin.mkdir()
        # Inert CLI shim logs argv and reads real bracketed-paste bytes using cat.
        for name in ('claude', 'codex'):
            cli = self.bin / name
            cli.write_text("""#!/bin/sh
name=$(basename "$0")
printf '%s\\n' "$@" > "$BRIDGE_TEST_RECORDS/$name.args"
printf 'NO_COLOR=%s\\n' "${NO_COLOR-unset}" >> "$BRIDGE_TEST_RECORDS/$name.args"
stty raw -echo
printf '\\033[?2004h%s ready\\r\\n' "$name"
exec /bin/cat > "$BRIDGE_TEST_RECORDS/$name.input"
""")
            cli.chmod(0o755)
        self.socket = self.base / 's'
        wrapper = self.bin / 'tmux'
        command = shlex.quote(TMUX) + ' -S ' + shlex.quote(str(self.socket)) + ' -f /dev/null'
        # Only process identity is stubbed: the actual terminal reader is cat.
        # All session metadata, pane selection, capture and paste go to real tmux.
        wrapper.write_text("""#!/bin/sh
last=''
for arg in "$@"; do last=$arg; done
if [ "$1" = display-message ] && [ "$last" = '#{pane_current_command}' ]; then
  actual=$(__TMUX__ "$@") || exit 1
  if [ "$actual" = cat ]; then
    target=''
    take=0
    for arg in "$@"; do
      if [ "$take" = 1 ]; then target=$arg; take=0; fi
      if [ "$arg" = -t ]; then take=1; fi
    done
    session=$(__TMUX__ display-message -p -t "$target" '#{session_name}') || exit 1
    case "$session" in
      tc-*) printf 'claude\\n'; exit 0 ;;
      ta-codex-*) printf 'codex\\n'; exit 0 ;;
    esac
  fi
  printf '%s\\n' "$actual"
  exit 0
fi
exec __TMUX__ "$@"
""".replace('__TMUX__', command))
        wrapper.chmod(0o755)
        home = self.base / 'home'
        home.mkdir()
        # Only test-child environment is replaced; user configuration is never edited.
        self.env = {'PATH': str(self.bin) + os.pathsep + os.defpath, 'HOME': str(home),
                    'TERM': 'xterm-256color', 'NO_COLOR': '1', 'BRIDGE_TEST_RECORDS': str(self.base)}

    def tearDown(self):
        subprocess.run([TMUX, '-S', str(self.socket), 'kill-server'], capture_output=True)
        self.tmp.cleanup()

    def run_helper(self, agent, *args, ok=True, data=None, cwd=None):
        p = subprocess.run(['/bin/bash', str(SCRIPTS / 'tmux-agent.sh'), agent, *args],
                           cwd=cwd or self.project, env=self.env, input=data,
                           text=True, capture_output=True, timeout=10)
        if ok:
            self.assertEqual(p.returncode, 0, p.stderr)
        else:
            self.assertNotEqual(p.returncode, 0, p.stdout)
        return p

    def tmux(self, *args):
        # Test-side exact session resolution, including show-options which rejects =name.
        args = list(args)
        for i, arg in enumerate(args):
            if arg.startswith('='):
                rows = subprocess.run([TMUX, '-S', str(self.socket), 'list-sessions', '-F', '#{session_id} #{session_name}'],
                                      env=self.env, text=True, capture_output=True, check=True).stdout.splitlines()
                args[i] = next(row.split(' ', 1)[0] for row in rows if row.split(' ', 1)[1] == arg[1:])
        return subprocess.run([TMUX, '-S', str(self.socket), '-f', '/dev/null', *args], env=self.env,
                              text=True, capture_output=True, check=True).stdout.strip()

    def start(self, agent):
        session = self.run_helper(agent, 'name').stdout.strip()
        self.run_helper(agent, 'start', str(self.project))
        target = self.run_helper(agent, 'target', session).stdout.strip()
        for _ in range(100):
            if agent + ' ready' in self.run_helper(agent, 'capture', target).stdout:
                return session, target
            time.sleep(.02)
        self.fail('inert CLI did not become ready')

    def test_names_and_compatibility_wrapper(self):
        for agent, prefix in [('claude', 'tc'), ('codex', 'ta-codex')]:
            actual = self.run_helper(agent, 'format-name', 'acme%:web', 'release/1.0').stdout.strip()
            self.assertEqual(actual, prefix + '-acme%25%3Aweb-release/1%2E0')
        result = subprocess.run(['/bin/bash', str(SCRIPTS / 'tmux-claude.sh'), 'name'],
                                cwd=self.project, env=self.env, text=True, capture_output=True, check=True)
        self.assertEqual(result.stdout, self.run_helper('claude', 'name').stdout)
        self.assertFalse(self.socket.exists())

    def test_process_contract_and_pure_name_without_tmux(self):
        for agent in ['claude', 'codex']:
            for command in ['claude', 'codex', 'bash', 'node', 'python']:
                result = subprocess.run(['/bin/bash', '-c', 'source "$1"; adapter_is_process "$2"',
                                         'test', str(SCRIPTS / 'adapters' / (agent + '.sh')), command],
                                        env=self.env, capture_output=True)
                self.assertEqual(result.returncode == 0, command == agent)
        env = dict(self.env, PATH='/usr/bin:/bin')
        result = subprocess.run(['/bin/bash', str(SCRIPTS / 'tmux-agent.sh'), 'codex', 'format-name', 'project'],
                                env=env, text=True, capture_output=True, check=True)
        self.assertEqual(result.stdout.strip(), 'ta-codex-project-main')

    def test_unknown_adapter_is_rejected_without_start(self):
        for name in ['missing', '../lib/core', 'codex;echo unsafe']:
            self.assertIn('unsupported agent', self.run_helper(name, 'start', ok=False).stderr)
        self.assertFalse(self.socket.exists())

    def test_both_targets_start_reuse_and_contracts(self):
        claude, _ = self.start('claude')
        codex, _ = self.start('codex')
        self.assertNotEqual(claude, codex)
        self.assertIn('reused agent=codex', self.run_helper('codex', 'start').stdout)
        self.assertEqual(len(self.tmux('list-sessions', '-F', '#{session_name}').splitlines()), 2)
        self.assertEqual((self.base / 'codex.args').read_text().splitlines(),
                         ['--no-alt-screen', '--sandbox', 'workspace-write', '--ask-for-approval', 'never', 'NO_COLOR=unset'])
        args = (self.base / 'claude.args').read_text().splitlines()
        self.assertEqual(args[:3], ['--permission-mode', 'bypassPermissions', '--session-id'])
        self.assertRegex(args[3], r'^[a-f0-9-]{36}$')
        self.assertEqual(args[4], 'NO_COLOR=unset')
        self.assertEqual(self.tmux('show-options', '-v', '-t', '=' + claude, '@tmux_claude_session_id'), args[3])
        self.assertEqual(self.tmux('show-options', '-v', '-t', '=' + codex, '@tmux_codex_session_id'), '')

    def test_multiline_is_pasted_once_and_not_executed(self):
        for agent in ['claude', 'codex']:
            _, target = self.start(agent)
            message = '[CALLER→' + agent.upper() + '][REVIEW][READ_ONLY]\nhello\n$(touch NEVER) `echo nope` % : 中文'
            self.run_helper(agent, 'send', target, data=message)
            expected = b'\x1b[200~' + message.encode() + b'\x1b[201~\r'
            for _ in range(100):
                received = (self.base / (agent + '.input')).read_bytes()
                if received == expected:
                    break
                time.sleep(.02)
            self.assertEqual(received, expected)
            self.assertFalse((self.project / 'NEVER').exists())

    def test_legacy_claude_uuid_required_but_pane_metadata_optional(self):
        session, target = self.start('claude')
        self.tmux('set-option', '-u', '-t', '=' + session, '@tmux_claude_pane_id')
        self.assertEqual(self.run_helper('claude', 'target', session).stdout.strip(), target)
        self.assertIn('reused', self.run_helper('claude', 'start').stdout)
        self.tmux('set-option', '-u', '-t', '=' + session, '@tmux_claude_session_id')
        self.assertIn('session id missing or invalid', self.run_helper('claude', 'send', target, 'hello', ok=False).stderr)
        self.assertIn('session id missing or invalid', self.run_helper('claude', 'start', ok=False).stderr)
        self.run_helper('claude', 'capture', target)
        self.run_helper('claude', 'status', target)

    def test_legacy_wrapper_reuses_uuid_and_preserves_multiline_send(self):
        session, target = self.start('claude')
        native_id = self.tmux('show-options', '-v', '-t', '=' + session, '@tmux_claude_session_id')
        original_args = (self.base / 'claude.args').read_bytes()
        self.tmux('set-option', '-u', '-t', '=' + session, '@tmux_claude_pane_id')
        wrapper = ['/bin/bash', str(SCRIPTS / 'tmux-claude.sh')]
        for args in [('start',), ('target', session), ('status', target)]:
            subprocess.run(wrapper + list(args), cwd=self.project, env=self.env,
                           capture_output=True, text=True, check=True)
        self.assertEqual(self.tmux('show-options', '-v', '-t', '=' + session, '@tmux_claude_session_id'), native_id)
        self.assertEqual((self.base / 'claude.args').read_bytes(), original_args)
        message = '[CALLER→CLAUDE][REVIEW][READ_ONLY]\n旧入口继续可用\n只读、不修改'
        subprocess.run(wrapper + ['send', target], input=message, cwd=self.project, env=self.env,
                       capture_output=True, text=True, check=True)
        expected = b'\x1b[200~' + message.encode() + b'\x1b[201~\r'
        for _ in range(100):
            received = (self.base / 'claude.input').read_bytes()
            if received == expected:
                break
            time.sleep(.02)
        self.assertEqual(received, expected)
        self.tmux('set-option', '-t', '=' + session, '@tmux_claude_permission_mode', 'default')
        self.assertIn('permission mode mismatch', self.run_helper('claude', 'start', ok=False).stderr)
        self.run_helper('claude', 'send', target, 'must not send', ok=False)
        self.assertEqual((self.base / 'claude.input').read_bytes(), expected)

    def test_immediate_cli_exit_reports_failure_without_success(self):
        for agent in ['claude', 'codex']:
            with self.subTest(agent=agent):
                (self.bin / agent).write_text('#!/bin/sh\nexit 17\n')
                result = self.run_helper(agent, 'start', ok=False)
                self.assertIn('failed to start agent=' + agent, result.stderr)
                self.assertIn('session=' + self.run_helper(agent, 'name').stdout.strip(), result.stderr)
                self.assertNotIn('started agent=', result.stdout)

    def test_dead_pane_retained_by_tmux_is_not_reported_started(self):
        self.tmux('new-session', '-d', '-s', 'test-keeper', 'sleep 60')
        self.tmux('set-option', '-g', 'remain-on-exit', 'on')
        for agent in ['claude', 'codex']:
            with self.subTest(agent=agent):
                (self.bin / agent).write_text('#!/bin/sh\nexit 17\n')
                result = self.run_helper(agent, 'start', ok=False)
                self.assertIn('failed to start agent=' + agent, result.stderr)
                self.assertNotIn('started agent=', result.stdout)
                session = self.run_helper(agent, 'name').stdout.strip()
                self.assertIn(session, self.tmux('list-sessions', '-F', '#{session_name}'))
                reused = self.run_helper(agent, 'start', ok=False)
                self.assertIn('refusing dead or unavailable pane', reused.stderr)
                self.assertNotIn('reused agent=', reused.stdout)
                self.run_helper(agent, 'target', session, ok=False)
                # Still allow read-only diagnostics of the retained dead pane.
                self.run_helper(agent, 'capture', session + ':0.0')
                self.run_helper(agent, 'status', session + ':0.0')
                if agent == 'claude':
                    self.tmux('set-option', '-u', '-t', '=' + session, '@tmux_claude_pane_id')
                    self.assertIn('refusing dead or unavailable pane',
                                  self.run_helper(agent, 'start', ok=False).stderr)

    def disappear_during_lookup(self, session, lookup_count):
        wrapper = self.bin / 'tmux'
        original = wrapper.read_text()
        counter = shlex.quote(str(self.base / 'lookup-count'))
        real = shlex.quote(TMUX) + ' -S ' + shlex.quote(str(self.socket)) + ' -f /dev/null'
        # Simulate the reviewed race on our isolated server; never touch user tmux.
        guard = f'''
if [ "$1" = list-sessions ]; then
  n=0
  if [ -f {counter} ]; then n=$(cat {counter}); fi
  n=$((n + 1)); printf '%s' "$n" > {counter}
  if [ "$n" = {lookup_count} ]; then {real} kill-session -t {shlex.quote(session)}; fi
fi
if [ "$1" = attach-session ] || [ "$1" = switch-client ]; then
  printf '%s' "$1" > {shlex.quote(str(self.base / 'unexpected-attach'))}
  exit 0
fi
'''
        wrapper.write_text(original.replace('#!/bin/sh\n', '#!/bin/sh\n' + guard, 1))
        return original

    def test_destroy_race_preserves_unrelated_session(self):
        session, _ = self.start('claude')
        self.tmux('new-session', '-d', '-s', 'unrelated-survivor', 'sleep 60')
        self.disappear_during_lookup(session, 3)
        result = self.run_helper('claude', 'destroy', session, ok=False)
        self.assertIn('refusing unresolved session handle', result.stderr)
        self.assertNotIn('destroyed session=', result.stdout)
        self.assertEqual(self.tmux('list-sessions', '-F', '#{session_name}'), 'unrelated-survivor')

    def test_attach_race_never_uses_default_target(self):
        for inside_tmux in [False, True]:
            with self.subTest(inside_tmux=inside_tmux):
                session, _ = self.start('claude')
                counter = self.base / 'lookup-count'
                counter.unlink(missing_ok=True)
                original = self.disappear_during_lookup(session, 2)
                if inside_tmux:
                    self.env['TMUX'] = 'test-client'
                try:
                    result = self.run_helper('claude', 'attach', session, ok=False)
                    self.assertIn('refusing unresolved session handle', result.stderr)
                    self.assertFalse((self.base / 'unexpected-attach').exists())
                finally:
                    self.env.pop('TMUX', None)
                    (self.bin / 'tmux').write_text(original)

    def test_reference_names_do_not_become_automatic_agent_rules(self):
        references = SCRIPTS.parent / 'references'
        self.assertTrue((references / 'claude-cli.md').is_file())
        self.assertFalse(any(p.name.lower() in {'claude.md', 'agents.md'} for p in references.iterdir()))
        self.assertNotIn('references/claude.md', (SCRIPTS.parent / 'SKILL.md').read_text())

    def test_mismatched_provider_permission_and_root_refuse_without_input(self):
        session, target = self.start('codex')
        self.run_helper('claude', 'target', session, ok=False)
        self.run_helper('claude', 'send', target, 'hello', ok=False)
        other = self.base / 'other'
        other.mkdir()
        self.assertIn('project root mismatch', self.run_helper('codex', 'send', target, 'hello', cwd=other, ok=False).stderr)
        self.tmux('set-option', '-t', '=' + session, '@tmux_codex_permission_mode', 'wrong')
        self.assertIn('permission mode mismatch', self.run_helper('codex', 'start', ok=False).stderr)
        self.run_helper('codex', 'send', target, 'hello', ok=False)
        self.assertEqual((self.base / 'codex.input').read_bytes(), b'')

    def test_pane_pinning_and_legacy_ambiguity(self):
        session, target = self.start('codex')
        self.tmux('new-window', '-d', '-t', '=' + session, 'sleep 60')
        self.assertEqual(self.run_helper('codex', 'target', session).stdout.strip(), target)
        self.tmux('set-option', '-u', '-t', '=' + session, '@tmux_codex_pane_id')
        self.assertIn('ambiguous', self.run_helper('codex', 'target', session, ok=False).stderr)
        self.run_helper('codex', 'send', target, 'hello', ok=False)

    def test_unmanaged_and_other_process_refuse(self):
        session = self.run_helper('codex', 'name').stdout.strip()
        target = self.tmux('new-session', '-d', '-P', '-F', '#{session_name}:#{window_index}.#{pane_index}',
                           '-s', session, 'sleep 60')
        self.assertIn('unrecognized session', self.run_helper('codex', 'start', ok=False).stderr)
        self.run_helper('codex', 'send', target, 'hello', ok=False)
        self.run_helper('codex', 'destroy', session, ok=False)
        self.assertIn(session, self.tmux('list-sessions', '-F', '#{session_name}'))

    def test_empty_message_bad_capture_and_prefix_target_rejected(self):
        session, target = self.start('codex')
        self.run_helper('codex', 'send', target, data='', ok=False)
        self.run_helper('codex', 'capture', target, '5001', ok=False)
        self.run_helper('codex', 'send', 'ta-codex', 'hello', ok=False)
        self.assertEqual((self.base / 'codex.input').read_bytes(), b'')
        self.assertIn('codex ready', self.run_helper('codex', 'capture', target, '5000').stdout)

    def test_destroy_only_selected_adapter(self):
        claude, _ = self.start('claude')
        codex, _ = self.start('codex')
        self.run_helper('claude', 'destroy', codex, ok=False)
        self.run_helper('codex', 'destroy', codex)
        self.assertEqual(self.tmux('list-sessions', '-F', '#{session_name}'), claude)


if __name__ == '__main__':
    unittest.main(verbosity=2)
