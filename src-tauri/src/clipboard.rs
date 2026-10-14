/// 在 App 主线程内写入明确的纯文本类型，并读回核验。
/// 不再把外部 pbcopy 退出成功当作写入成功，也不让它猜测 RTF/EPS 格式。
#[cfg(target_os = "macos")]
pub fn write_text(text: &str) -> Result<(), String> {
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSPasteboard;
    use objc2_foundation::NSString;

    let _main_thread = MainThreadMarker::new()
        .ok_or_else(|| "剪贴板操作必须在 App 主线程执行".to_string())?;
    if text.is_empty() {
        return Err("没有可复制的文字".into());
    }

    let board = NSPasteboard::generalPasteboard();
    let plain_text = NSString::from_str("public.utf8-plain-text");
    let value = NSString::from_str(text);
    board.clearContents();
    if !board.setString_forType(&value, &plain_text) {
        return Err("系统拒绝写入剪贴板".into());
    }
    let actual = board.stringForType(&plain_text).map(|value| value.to_string());
    verify_text(text, actual.as_deref())
}

#[cfg(any(target_os = "macos", test))]
fn verify_text(expected: &str, actual: Option<&str>) -> Result<(), String> {
    if expected.is_empty() {
        return Err("没有可复制的文字".into());
    }
    let actual = actual.ok_or_else(|| "剪贴板写入后未读到纯文本".to_string())?;
    if actual != expected {
        return Err(format!(
            "剪贴板核验失败（期望 {} 字节，实际 {} 字节）",
            expected.len(),
            actual.len()
        ));
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub fn write_text(_text: &str) -> Result<(), String> {
    Err("当前平台不支持此原生剪贴板通道".into())
}

#[cfg(test)]
mod tests {
    use super::verify_text;

    #[test]
    fn verifies_exact_plain_text() {
        for text in ["中文🙂\n第二行", "{\\rtf1 literal}", "%!PS-Adobe-3.0", "  \n"] {
            assert!(verify_text(text, Some(text)).is_ok());
        }
    }

    #[test]
    fn rejects_missing_empty_or_changed_readback() {
        assert!(verify_text("原文", None).is_err());
        assert!(verify_text("原文", Some("")).is_err());
        assert!(verify_text("原文", Some("其他内容")).is_err());
        assert!(verify_text("", Some("")).is_err());
    }
}
