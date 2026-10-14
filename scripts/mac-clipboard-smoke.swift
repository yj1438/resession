import AppKit

// 只写隔离的临时 pasteboard，不读取或覆盖用户的 general 剪贴板。
// 验证候选原生 API 的纯文本往返，不替代真实 ReSession.app 的手动验收。
func run() throws {
    let board = NSPasteboard.withUniqueName()
    defer { board.releaseGlobally() }
    let cases: [(String, String)] = [
        ("中文多行", "第一行中文\n第二行"),
        ("Emoji", "复制🙂👨‍👩‍👧‍👦"),
        ("RTF 开头仍为原文", "{\\rtf1\\ansi literal text}"),
        ("EPS 开头仍为原文", "%!PS-Adobe-3.0\n%%原样文字"),
        ("空格与换行", "  \n\t  "),
        ("组合字符", "e\u{301} İ"),
        ("长文本", String(repeating: "中文🙂\n", count: 10_000)),
    ]
    for (name, text) in cases {
        board.clearContents()
        guard board.setString(text, forType: .string),
              let actual = board.string(forType: .string),
              actual.utf8.elementsEqual(text.utf8) else {
            throw NSError(domain: "ReSessionClipboardSmoke", code: 1,
                          userInfo: [NSLocalizedDescriptionKey: "核验失败：\(name)"])
        }
        print("通过：\(name)，\(text.utf8.count) 字节")
    }
    print("7 组隔离原生往返通过；真实 App / Cmd+C / 菜单 / 按钮仍须实测。")
}

do {
    try run()
} catch {
    fputs("\(error.localizedDescription)\n", stderr)
    exit(1)
}
