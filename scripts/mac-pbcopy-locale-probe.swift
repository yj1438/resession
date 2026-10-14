import AppKit

// 只用固定样本比较 pbcopy 的 locale；不启动 Agent、不输出原剪贴板正文。
// 暂用 general 剪贴板，保存原数据于内存，期间有外部改动则不恢复覆盖。
func run() throws {
    let board = NSPasteboard.general
    let saved = (board.pasteboardItems ?? []).map { item in
        item.types.compactMap { type in item.data(forType: type).map { (type, $0) } }
    }
    var ownedCount = board.changeCount
    defer {
        if board.changeCount == ownedCount {
            board.clearContents()
            let items = saved.map { values -> NSPasteboardItem in
                let item = NSPasteboardItem()
                for (type, data) in values { item.setData(data, forType: type) }
                return item
            }
            if !items.isEmpty { board.writeObjects(items) }
            print("已恢复原剪贴板（未输出正文）")
        } else { print("外部剪贴板已变化，未覆盖") }
    }
    for locale in ["unset", "C", "UTF-8"] {
        for (label, text) in [("ASCII", "RS-COPY"), ("中文", "中文测试"), ("Emoji", "🙂")] {
            guard board.changeCount == ownedCount else {
                throw NSError(domain: "ClipboardProbeExternalChange", code: 1)
            }
            let task = Process()
            task.executableURL = URL(fileURLWithPath: "/usr/bin/pbcopy")
            var environment = ProcessInfo.processInfo.environment
            for key in ["LANG", "LC_ALL", "LC_CTYPE"] { environment.removeValue(forKey: key) }
            if locale == "C" { environment["LC_ALL"] = "C" }
            if locale == "UTF-8" { environment["LANG"] = "en_US.UTF-8" }
            task.environment = environment
            let input = Pipe()
            task.standardInput = input
            try task.run()
            try input.fileHandleForWriting.write(contentsOf: Data(text.utf8))
            try input.fileHandleForWriting.close()
            task.waitUntilExit()
            let actual = board.string(forType: .string)
            guard actual == text || actual == "" || actual == nil else {
                throw NSError(domain: "ClipboardProbeUnexpectedContent", code: 1)
            }
            ownedCount = board.changeCount
            print("locale=\(locale) 样本=\(label) exit=\(task.terminationStatus) 输入=\(text.utf8.count)字节 纯文本=\(board.data(forType: .string)?.count ?? -1)字节 一致=\(actual == text)")
        }
    }
}

do { try run() }
catch { fputs("探针中止：\(error.localizedDescription)\n", stderr); exit(1) }
