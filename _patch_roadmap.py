import pathlib
p = pathlib.Path("F:/git-workspace/ai/resession/docs/roadmap.md")
s = p.read_text(encoding="utf-8")
old = "- [x] 增加坏 JSONL、缺字段、目录失效、二进制不存在等测试；"
addition = """
- [x] macOS GUI 环境修复：Homebrew/npm/版本管理器候选路径（platform::extra_bin_dirs）+ 登录 shell 环境解析（login_shell_path，探测与 PTY 环境基座双接入）；待 Mac 实机验收（docs/mac-verification.md）；"""
new = old + addition
if new in s:
    print("skip: already ticked")
elif old in s:
    p.write_text(s.replace(old, new, 1), encoding="utf-8", newline="\n")
    print("roadmap updated")
else:
    raise SystemExit("MARKER NOT FOUND in roadmap")
