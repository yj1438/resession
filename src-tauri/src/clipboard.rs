/// macOS 的 WebView 复制命令可能返回成功但未写入系统剪贴板。
/// 使用系统自带 pbcopy；选中文字仅经 stdin 传入，不拼进 shell 命令或日志。
#[cfg(target_os = "macos")]
pub fn write_text(text: &str) -> Result<(), String> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let mut child = Command::new("/usr/bin/pbcopy")
        .args(["-pboard", "general"])
        // Finder 启动可能没有 UTF-8 locale，明确指定以保留中文。
        .env("LC_ALL", "en_US.UTF-8")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("无法打开系统剪贴板：{error}"))?;

    let written = child
        .stdin
        .take()
        .ok_or_else(|| "无法写入系统剪贴板".to_string())
        .and_then(|mut input| {
            input
                .write_all(text.as_bytes())
                .map_err(|error| format!("剪贴板写入失败：{error}"))
        });
    // stdin 在等待前关闭，pbcopy 收到 EOF 后才会提交剪贴板。
    if let Err(error) = written {
        let _ = child.kill();
        let _ = child.wait();
        return Err(error);
    }
    let result = child.wait().map_err(|error| format!("剪贴板写入失败：{error}"))?;
    if result.success() {
        Ok(())
    } else {
        Err(format!("系统剪贴板写入失败（退出状态：{result}）"))
    }
}

#[cfg(not(target_os = "macos"))]
pub fn write_text(_text: &str) -> Result<(), String> {
    Err("当前平台不支持此原生剪贴板通道".into())
}
