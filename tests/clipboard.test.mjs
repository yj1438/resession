import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import vm from "node:vm";
import ts from "typescript";

// 使用项目已有 TypeScript 编译器；不安装浏览器测试框架、不访问真实剪贴板。
function loadSource(path, globals = {}, dependencies = {}) {
  const source = readFileSync(new URL(path, import.meta.url), "utf8");
  const { outputText } = ts.transpileModule(source, {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 },
  });
  const exports = {};
  vm.runInNewContext(outputText, {
    exports,
    require(id) {
      if (!(id in dependencies)) throw new Error(`未配置测试依赖：${id}`);
      return dependencies[id];
    },
    ...globals,
  });
  return exports;
}

function clipboardEnvironment({ native = true, syncCopy = true, invoke } = {}) {
  const calls = [];
  class Element {
    focus() {}
  }
  const globals = {
    navigator: { platform: "MacIntel", clipboard: { async writeText(text) { calls.push(text); } } },
    HTMLElement: Element,
    TextEncoder,
    document: {
      activeElement: new Element(),
      body: { appendChild() {} },
      createElement() {
        return { style: {}, focus() {}, select() {}, remove() {} };
      },
      execCommand() { return syncCopy; },
    },
  };
  const api = {
    isTauri: native,
    invoke: invoke ?? (async (command, args) => { calls.push({ command, text: args.text }); }),
  };
  return { calls, globals, copyText: loadSource("../src/clipboard.ts", globals, { "./api": api }).copyText };
}

test("Mac App 只走原生核验通道，保留中文、多行和特殊格式开头", async () => {
  for (const text of ["中文🙂\n第二行", "{\\rtf1 literal}", "%!PS-Adobe-3.0\n原文", "  空白  "]) {
    const env = clipboardEnvironment();
    assert.equal(await env.copyText(text), "native-verified");
    assert.deepEqual(env.calls, [{ command: "copy_text", text }]);
  }
});

test("原生核验失败不回退到可能假成功的 WebView 通道", async () => {
  const env = clipboardEnvironment({ invoke: async () => { throw new Error("剪贴板核验失败"); } });
  await assert.rejects(env.copyText("待复制"), /核验失败/);
  assert.deepEqual(env.calls, []);
});

test("原生请求传递入口和 UTF-8 字节数，不把字符数当字节数", async () => {
  let received;
  const env = clipboardEnvironment({ invoke: async (_, args) => { received = args; } });
  await env.copyText("中文🙂", "key");
  assert.equal(received.source, "key");
  assert.equal(received.expectedBytes, 10);
  assert.equal(received.text, "中文🙂");
});

test("空选区不清空剪贴板", async () => {
  const env = clipboardEnvironment();
  await assert.rejects(env.copyText(""), /没有可复制/);
  assert.deepEqual(env.calls, []);
});

test("浏览器同步复制保留原路径", async () => {
  const env = clipboardEnvironment({ native: false });
  assert.equal(await env.copyText("浏览器"), "browser");
  assert.deepEqual(env.calls, []);
});

test("浏览器同步失败时回退异步写入", async () => {
  const env = clipboardEnvironment({ native: false, syncCopy: false });
  assert.equal(await env.copyText("浏览器回退"), "browser");
  assert.deepEqual(env.calls, ["浏览器回退"]);
});

const { bindTerminalCopy } = loadSource("../src/terminalClipboard.ts");

function terminalEnvironment(isMac = true, nativeCopy = isMac) {
  const host = new EventTarget();
  const copied = [];
  let keyHandler;
  let selection = "终端选区";
  const term = {
    getSelection: () => selection,
    attachCustomKeyEventHandler: (handler) => { keyHandler = handler; },
  };
  const dispose = bindTerminalCopy(host, term, isMac, (text) => copied.push(text), nativeCopy);
  return {
    host, copied, dispose,
    setSelection(text) { selection = text; },
    key(overrides = {}) {
      const event = {
        type: "keydown", key: "c", metaKey: isMac, ctrlKey: !isMac,
        shiftKey: !isMac, altKey: false, defaultPrevented: false,
        preventDefault() { this.defaultPrevented = true; }, ...overrides,
      };
      return { result: keyHandler(event), event };
    },
  };
}

test("Mac Cmd+C 使用终端选区且阻止默认复制", () => {
  const env = terminalEnvironment();
  const { result, event } = env.key();
  assert.equal(result, false);
  assert.equal(event.defaultPrevented, true);
  assert.deepEqual(env.copied, ["终端选区"]);
  env.dispose();
});

test("菜单 copy 事件走同一路径并阻止后续浏览器处理", () => {
  const env = terminalEnvironment();
  let browserCopies = 0;
  env.host.addEventListener("copy", () => { browserCopies += 1; });
  const event = new Event("copy", { cancelable: true });
  env.host.dispatchEvent(event);
  assert.equal(event.defaultPrevented, true);
  assert.equal(browserCopies, 0);
  assert.deepEqual(env.copied, ["终端选区"]);
  env.dispose();
});

test("无选区时不接管 Cmd+C 或菜单 copy", () => {
  const env = terminalEnvironment();
  env.setSelection("");
  assert.equal(env.key().result, true);
  const event = new Event("copy", { cancelable: true });
  env.host.dispatchEvent(event);
  assert.equal(event.defaultPrevented, false);
  assert.deepEqual(env.copied, []);
  env.dispose();
});

test("Ctrl+C、中断、粘贴和 keyup 保留终端默认行为", () => {
  const env = terminalEnvironment();
  for (const overrides of [
    { metaKey: false, ctrlKey: true },
    { key: "v" },
    { altKey: true },
    { type: "keyup" },
  ]) assert.equal(env.key(overrides).result, true);
  assert.deepEqual(env.copied, []);
  env.dispose();
});

test("Windows/Linux 仍使用 Ctrl+Shift+C，不抢占 Ctrl+C", () => {
  const env = terminalEnvironment(false);
  assert.equal(env.key({ shiftKey: false }).result, true);
  assert.equal(env.key().result, false);
  assert.deepEqual(env.copied, ["终端选区"]);
  env.dispose();
});

test("终端卸载后移除复制监听", () => {
  const env = terminalEnvironment();
  env.dispose();
  const event = new Event("copy", { cancelable: true });
  env.host.dispatchEvent(event);
  assert.equal(event.defaultPrevented, false);
  assert.equal(env.key().result, true);
  assert.deepEqual(env.copied, []);
});

test("浏览器和非 Mac 不接管原有 DOM copy 路径", () => {
  const env = terminalEnvironment(true, false);
  const event = new Event("copy", { cancelable: true });
  env.host.dispatchEvent(event);
  assert.equal(event.defaultPrevented, false);
  assert.deepEqual(env.copied, []);
  env.dispose();
});
