import pathlib
p = pathlib.Path("F:/git-workspace/ai/resession/src-tauri/src/pty.rs")
s = p.read_text(encoding="utf-8")
old = """    cmd.env_clear();
    for (k, v) in std::env::vars_os() {
        let name = k.to_string_lossy().to_string();
        let upper = name.to_uppercase();
        // CLAUDE*：child-session 标记会关闭转录保存；
        // CI / NO_COLOR：两者都会压掉 TUI 颜色
        if upper.starts_with("CLAUDE")
            || (upper.starts_with("CODEX_") && upper != "CODEX_HOME")
            || upper == "CI"
            || upper == "NO_COLOR"
        {
            continue;
        }
        cmd.env(&name, v);
    }
"""
new = """    cmd.env_clear();
    let mut app_path: Option<String> = None;
    for (k, v) in std::env::vars_os() {
        let name = k.to_string_lossy().to_string();
        let upper = name.to_uppercase();
        // CLAUDE*：child-session 标记会关闭转录保存；
        // CI / NO_COLOR：两者都会压掉 TUI 颜色
        if upper.starts_with("CLAUDE")
            || (upper.starts_with("CODEX_") && upper != "CODEX_HOME")
            || upper == "CI"
            || upper == "NO_COLOR"
        {
            continue;
        }
        if upper == "PATH" {
            app_path = Some(v.to_string_lossy().into_owned());
        }
        cmd.env(&name, v);
    }
    // macOS GUI 启动时 app 的 PATH 残缺：把登录 shell 解析出的完整 PATH
    // 前置合并，恢复出来的会话里 node/gh 等 CLI 工具才能正常使用。
    // 其它平台 login_shell_path 返回 None，行为不变。
    if let Some(shell_path) = crate::platform::login_shell_path() {
        let merged = match &app_path {
            Some(p) if !p.is_empty() => format!("{}:{}", shell_path, p),
            _ => shell_path,
        };
        cmd.env("PATH", merged);
    }
"""
if new in s:
    print("skip: already applied")
elif old in s:
    p.write_text(s.replace(old, new, 1), encoding="utf-8", newline="\n")
    print("patched: pty.rs")
else:
    raise SystemExit("MARKER NOT FOUND in pty.rs")
