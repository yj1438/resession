# 适配器契约

`tmux-agent.sh <agent> <command>` 只加载本 Plugin `scripts/adapters/<agent>.sh`。agent 必须匹配 `[a-z][a-z0-9-]*`；文件不存在则拒绝。不能传绝对路径、shell 表达式或运行期下载适配器。

公共层 `scripts/lib/core.sh` 负责名称编码、项目归属、精确查找、pane 锁定、buffer 传输、capture/status/attach/destroy。不要复制这些操作到适配器。

适配器声明：

| 字段 | 用途 |
| --- | --- |
| `ADAPTER_CLI` | 启动前校验的可执行文件名 |
| `ADAPTER_PREFIX` | 独立 tmux session 前缀 |
| `ADAPTER_METADATA` | 独立 tmux option 命名空间，不含 `@` |
| `ADAPTER_PERMISSION_MODE` | 复用/发送时核验的权限契约 |

适配器提供三个函数：

- `adapter_prepare_start`：设置 `ADAPTER_ARGV` Bash 数组及 `AGENT_SESSION_ID`（无法准确提供则为空）。只准备数据，不启动 tmux，不执行任务。
- `adapter_validate_session <session>`：使用公共 `session_option` 读取并核验该 CLI 特有 metadata。契约不符时输出完整 `refusing` 错误并失败；不得修补或销毁旧会话。
- `adapter_is_process <command>`：识别前台 CLI，匹配返回 0，不匹配返回非 0。不可宽泛接受 shell/node/python 等通用进程名。

公共层以 `@<namespace>_managed/project_root/permission_mode/session_id/pane_id` 保存状态；原 Claude namespace 不变以兼容旧会话。空 session_id 不代表空任务，而是该适配器不具备原生 ID 契约。

新增 CLI 时：新增适配器及目标 reference，更新 Skill 路由/README，增加启动、身份隔离和失败测试，以及对应行为 eval。启动参数、回复格式与取证降级规则只写在目标 reference；公共层不得出现按 CLI 名堆叠的分支。适配器是随 Plugin 分发的受信代码，不是第三方代码执行入口。

目标 reference 避免命名为 `claude.md`、`agents.md` 等与 Agent 规则文件仅大小写不同的名称；大小写不敏感文件系统会使其被自动加载。Claude 目标说明使用 `claude-cli.md`。
