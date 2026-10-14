@preconcurrency import AppKit
@preconcurrency import WebKit

// 用真实 WKWebView + 项目内 xterm 复现复制入口，不启动 CLI、不读取任何会话。
// general 剪贴板只暂写固定样本；原数据只保存在内存，检测到外部修改则不恢复覆盖。
@MainActor
final class ClipboardProbe: NSObject, WKScriptMessageHandler, WKNavigationDelegate, WKUIDelegate {
    let board = NSPasteboard.general
    let sentinel = "ReSession-clipboard-probe-sentinel"
    var backup: [[(NSPasteboard.PasteboardType, Data)]] = []
    var ownedCount: Int?
    var stageStartCount = 0
    var expected = ""
    var index = 0
    var finished = false
    var totals: [String: (passed: Int, failed: Int, skipped: Int)] = [:]
    var unavailableMenu = false
    var window: NSWindow!
    var view: WKWebView!
    let steps: [(String, String)] = [
        ("xterm", "select"), ("xterm", "menu"),
        ("release", "select"), ("release", "button"),
        ("release", "key"), ("release", "menu"),
        ("candidate", "select"), ("candidate", "button"),
        ("candidate", "key"), ("candidate", "menu"),
        ("race-empty", "menu"), ("race-filled", "menu"),
    ]
    let fixtures = [
        ("ASCII", "RS-COPY"),
        ("中文多行", "中文🙂\r\n第二行"),
        ("RTF 原文", "{\\rtf1 literal text}"),
        ("EPS 原文", "%!PS-Adobe-3.0 EPSF-3.0\r\n%%BoundingBox: 0 0 10 10"),
    ]
    var step: (String, String) { steps[index % steps.count] }

    func start() throws {
        backup = (board.pasteboardItems ?? []).map { item in
            item.types.compactMap { type in item.data(forType: type).map { (type, $0) } }
        }
        ownedCount = board.changeCount
        let controller = WKUserContentController()
        controller.add(self, name: "probe")
        let configuration = WKWebViewConfiguration()
        configuration.userContentController = controller
        view = WKWebView(frame: NSRect(x: 0, y: 0, width: 800, height: 320), configuration: configuration)
        view.navigationDelegate = self
        view.uiDelegate = self
        window = NSWindow(contentRect: view.frame, styleMask: [.titled], backing: .buffered, defer: false)
        window.title = "ReSession Clipboard Probe — 固定测试文本"
        window.contentView = view
        window.setFrameOrigin(NSPoint(x: -10000, y: -10000))
        window.orderBack(nil)
        window.makeFirstResponder(view)
        view.loadHTMLString(try html(), baseURL: nil)
        DispatchQueue.main.asyncAfter(deadline: .now() + 90) { [self] in
            if !finished { finish("WKWebView 探针超时", failed: true) }
        }
    }

    func html() throws -> String {
        let root = FileManager.default.currentDirectoryPath
        let source = try String(contentsOfFile: root + "/node_modules/@xterm/xterm/lib/xterm.js", encoding: .utf8)
        let css = try String(contentsOfFile: root + "/node_modules/@xterm/xterm/css/xterm.css", encoding: .utf8)
        let node = Process()
        node.executableURL = URL(fileURLWithPath: "/usr/bin/env")
        node.arguments = ["node", "--input-type=module", "-e", """
        import fs from 'node:fs'; import cp from 'node:child_process'; import ts from 'typescript';
        const compile=s=>ts.transpileModule(s,{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText;
        const release=cp.execFileSync('git',['show','v0.8.1:src/clipboard.ts'],{encoding:'utf8'});
        console.log(JSON.stringify({release:compile(release),candidate:compile(fs.readFileSync('src/clipboard.ts','utf8')),bind:compile(fs.readFileSync('src/terminalClipboard.ts','utf8'))}));
        """]
        let output = Pipe()
        node.standardOutput = output
        try node.run()
        let compiled = output.fileHandleForReading.readDataToEndOfFile()
        node.waitUntilExit()
        guard node.terminationStatus == 0, let modules = String(data: compiled, encoding: .utf8) else {
            throw NSError(domain: "ClipboardProbe", code: 1)
        }
        return """
        <!doctype html><meta charset="utf-8"><style>\(css)</style>
        <div id="host" style="width:760px;height:250px"></div><button id="copy">复制</button>
        <script>\(source.replacingOccurrences(of: "</script", with: "<\\/script"))</script>
        <script>
        const sources=\(modules), pending=new Map(); let sequence=0,term,unbind,mode,lastCopy;
        const send=x=>window.webkit.messageHandlers.probe.postMessage(x);
        const api={isTauri:true,invoke:(_,args)=>new Promise((resolve,reject)=>{
          const id=++sequence; pending.set(id,{resolve,reject});
          send({event:'write',id,text:args.text,expectedBytes:args.expectedBytes,
            method:mode==='release'?'pbcopy':'appkit'});
        })};
        window.nativeResult=(id,error)=>{const p=pending.get(id);pending.delete(id);error?p.reject(new Error(error)):p.resolve();};
        function module(source){const exports={};new Function('exports','require',source)(exports,()=>api);return exports;}
        const release=module(sources.release),candidate=module(sources.candidate),binder=module(sources.bind);
        const host=document.getElementById('host'),button=document.getElementById('copy');
        function copy(text,source='button'){lastCopy=(mode==='release'?release:candidate).copyText(text,source).then(
          ()=>send({event:'feedback',ok:true,bytes:new TextEncoder().encode(text).length}),
          error=>send({event:'feedback',ok:false,error:String(error)}));}
        host.addEventListener('copy',e=>send({event:'copy-before',trusted:e.isTrusted,
          selection:term.getSelection().length,data:e.clipboardData?.getData('text/plain').length??-1}),true);
        document.addEventListener('copy',e=>send({event:'copy-after',prevented:e.defaultPrevented,
          data:e.clipboardData?.getData('text/plain').length??-1}));
        window.prepare=(nextMode,action,fixture)=>{
          unbind?.();term?.dispose();host.innerHTML='';mode=nextMode;lastCopy=null;
          term=new Terminal({cols:70,rows:5,macOptionClickForcesSelection:true});term.open(host);
          if(mode==='candidate')unbind=binder.bindTerminalCopy(host,term,true,copy,true);
          else if(mode.startsWith('race-')){
            const handler=e=>{
              e.preventDefault();e.stopImmediatePropagation();
              if(mode==='race-filled')e.clipboardData?.setData('text/plain',term.getSelection());
              // 测试专用：通过 WKUIDelegate 让原生写入发生在 copy 事件返回前。
              // 不弹出真实对话框。这是人为强制时序，不代表实际 App 已抓到同一竞态。
              alert('probe-native-write');
            };
            host.addEventListener('copy',handler,true);
            unbind=()=>host.removeEventListener('copy',handler,true);
          }
          else if(mode==='release')term.attachCustomKeyEventHandler(e=>{
            if(e.type==='keydown'&&e.metaKey&&!e.ctrlKey&&!e.altKey&&/^[cC]$/.test(e.key)&&term.hasSelection()){
              e.preventDefault();copy(term.getSelection());return false;
            }return true;
          });
          button.onmousedown=mode==='candidate'?e=>e.preventDefault():null;
          button.onclick=()=>{const text=term.getSelection();if(text)copy(text);};
          term.write(fixture,()=>{
            term.focus();term.selectAll();
            send({event:'prepared',action,text:term.getSelection(),platform:navigator.platform});
          });
        };
        window.action=action=>{
          if(action==='button'){
            const down=new MouseEvent('mousedown',{bubbles:true,cancelable:true});
            if(button.dispatchEvent(down))button.focus();button.click();
          }else if(action==='key')term.textarea.dispatchEvent(new KeyboardEvent('keydown',{
            key:'c',code:'KeyC',metaKey:true,bubbles:true,cancelable:true}));
          window.finishStep();
        };
        window.finishStep=()=>Promise.resolve(lastCopy).then(()=>send({event:'done'}));
        send({event:'ready'});
        </script>
        """
    }

    func next() {
        if index >= steps.count * fixtures.count { finish("48 组探针完成（不是打包 App 验收）"); return }
        guard board.changeCount == ownedCount else { finish("检测到外部剪贴板修改，停止测试且保留新内容", failed: true); return }
        board.clearContents()
        board.setString(sentinel, forType: .string)
        ownedCount = board.changeCount
        stageStartCount = board.changeCount
        unavailableMenu = false
        let (mode, action) = step
        let fixture = fixtures[index / steps.count]
        let data = try! JSONSerialization.data(withJSONObject: [fixture.1])
        let literal = String(data: data, encoding: .utf8)!
        print("样本：\(fixture.0)")
        view.evaluateJavaScript("prepare('\(mode)','\(action)',\(literal)[0])") { [self] _, error in
            if let error { finish(error.localizedDescription, failed: true) }
        }
    }

    func userContentController(_ controller: WKUserContentController, didReceive message: WKScriptMessage) {
        guard let payload = message.body as? [String: Any], let event = payload["event"] as? String else { return }
        switch event {
        case "ready": next()
        case "prepared":
            expected = payload["text"] as? String ?? ""
            print("阶段：\(step.0)/\(step.1)，平台=\(payload["platform"] ?? "?")，选区=\(expected.utf8.count) 字节")
            if step.1 == "menu" {
                let selector = NSSelectorFromString("copy:")
                let handled = window.firstResponder?.tryToPerform(selector, with: nil) ?? false
                unavailableMenu = !handled
                print("原生菜单 action 被 responder 接受：\(handled)")
                DispatchQueue.main.asyncAfter(deadline: .now() + 0.15) { [self] in view.evaluateJavaScript("finishStep()", completionHandler: nil) }
            } else { view.evaluateJavaScript("action('\(step.1)')", completionHandler: nil) }
        case "write":
            let text = payload["text"] as? String ?? ""
            let id = payload["id"] as? Int ?? -1
            if let expectedBytes = payload["expectedBytes"] as? Int, expectedBytes != text.utf8.count {
                completeWrite(id, error: "前端与桥接收到的字节数不同")
                return
            }
            if payload["method"] as? String == "pbcopy" {
                DispatchQueue.global().async {
                    let error: String?
                    do { try Self.pbcopy(text); error = nil } catch let failure { error = failure.localizedDescription }
                    DispatchQueue.main.async { [self] in completeWrite(id, error: error) }
                }
            } else {
                board.clearContents()
                let ok = board.setString(text, forType: .string)
                let same = board.string(forType: .string) == text
                completeWrite(id, error: ok && same ? nil : "AppKit 写入/读回失败")
            }
        case "done":
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.2) { [self] in
                let actual = board.string(forType: .string)
                // pbcopy 会猜测 RTF/EPS 格式，可能没有可读的纯文本；作为失败观测保留。
                let isOwnValue = actual == expected || actual == sentinel || actual == "" || actual == nil
                guard isOwnValue else { finish("检测到非测试文本，保留外部剪贴板，不继续测试", failed: true); return }
                let changed = board.changeCount != stageStartCount
                ownedCount = board.changeCount
                let ok = step.1 == "select" ? actual == sentinel && !changed : actual == expected
                print("结果：\(unavailableMenu ? "无法执行菜单测试" : ok ? "通过" : "未通过")，变化=\(changed)，正文=\(actual?.utf8.count ?? -1) 字节，匹配选区=\(actual == expected)")
                print("格式：\((board.types ?? []).map { $0.rawValue }.joined(separator: ","))")
                var total = totals[step.0] ?? (0, 0, 0)
                if unavailableMenu { total.skipped += 1 }
                else if ok { total.passed += 1 }
                else { total.failed += 1 }
                totals[step.0] = total
                index += 1
                next()
            }
        default:
            // 只输出事件类型、布尔值与长度；不输出正文。
            print("事件：\(event)，trusted=\(payload["trusted"] ?? "-")，data=\(payload["data"] ?? "-")，ok=\(payload["ok"] ?? "-")，bytes=\(payload["bytes"] ?? "-")")
        }
    }

    func completeWrite(_ id: Int, error: String?) {
        ownedCount = board.changeCount
        let data = try! JSONSerialization.data(withJSONObject: [error as Any? ?? NSNull()])
        let array = String(data: data, encoding: .utf8)!
        view.evaluateJavaScript("nativeResult(\(id),\(array)[0])", completionHandler: nil)
    }

    func webView(_ webView: WKWebView, runJavaScriptAlertPanelWithMessage message: String,
                 initiatedByFrame frame: WKFrameInfo, completionHandler: @escaping () -> Void) {
        guard message == "probe-native-write" else { completionHandler(); return }
        board.clearContents()
        let ok = board.setString(expected, forType: .string)
        let matches = board.string(forType: .string) == expected
        ownedCount = board.changeCount
        print("强制时序：copy 事件返回前原生写入，结果=\(ok && matches)，正文=\(expected.utf8.count) 字节")
        completionHandler()
    }

    nonisolated static func pbcopy(_ text: String) throws {
        let task = Process()
        task.executableURL = URL(fileURLWithPath: "/usr/bin/pbcopy")
        task.arguments = ["-pboard", "general"]
        var environment = ProcessInfo.processInfo.environment
        environment["LC_ALL"] = "en_US.UTF-8"
        task.environment = environment
        let input = Pipe()
        task.standardInput = input
        try task.run()
        try input.fileHandleForWriting.write(contentsOf: Data(text.utf8))
        try input.fileHandleForWriting.close()
        task.waitUntilExit()
        if task.terminationStatus != 0 { throw NSError(domain: "pbcopy", code: Int(task.terminationStatus)) }
    }

    func finish(_ reason: String, failed: Bool = false) {
        guard !finished else { return }
        finished = true
        if board.changeCount == ownedCount {
            board.clearContents()
            let items = backup.map { saved -> NSPasteboardItem in
                let item = NSPasteboardItem()
                for (type, data) in saved { item.setData(data, forType: type) }
                return item
            }
            if !items.isEmpty { board.writeObjects(items) }
            print("已恢复原剪贴板（未输出正文）")
        } else { print("剪贴板已被其他操作更新，未覆盖") }
        print(reason)
        for key in totals.keys.sorted() {
            let total = totals[key]!
            print("汇总 \(key)：通过=\(total.passed)，未通过=\(total.failed)，跳过=\(total.skipped)")
        }
        window?.orderOut(nil)
        exit(failed ? 1 : 0)
    }
}

MainActor.assumeIsolated {
    let app = NSApplication.shared
    app.setActivationPolicy(.prohibited)
    let probe = ClipboardProbe()
    do { try probe.start(); app.run() }
    catch { probe.finish(error.localizedDescription, failed: true) }
}
