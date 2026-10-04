//! PTY 会话管理：portable-pty 生命周期 + 前端事件桥。
//!
//! 原则（architecture.md 3.3）：不解析、不记录、不干预终端内容。
//! 生命周期：PTY 以「provider:会话 id」为键，Agent 退出（读线程收割后移除句柄）
//! 或用户显式关闭（close()）时从运行列表消失；
//! UI 切换只断开观看，回来时回放缓冲区重新附着（支持并行多会话）。

use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use session_core::ResumeSpec;

/// 每个 PTY 保留的输出回放缓冲上限（字节）
const BUFFER_CAP: usize = 512 * 1024;
const FINISHED_SNAPSHOT_CAP: usize = 16;

pub struct PtyHandle {
    writer: Box<dyn Write + Send>,
    master: Box<dyn MasterPty + Send>,
    child: Arc<Mutex<Box<dyn Child + Send + Sync>>>,
    /// 原始字节缓冲：前端切走再切回时回放（字节级，避免多字节字符跨块损坏）
    buffer: Arc<Mutex<OutputBuffer>>,
    /// 最后一次输出的 Unix 毫秒（忙闲感知信号：有输出=活跃）
    last_output: Arc<Mutex<u64>>,
    /// 最后一次用户写入的 Unix 毫秒（区分"打字回显"与"真实响应"）
    last_write: Arc<Mutex<u64>>,
    cwd: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")] // 与前端 PtyStatus 镜像对齐（勿漏，同 SessionMeta）
pub struct PtyStatus {
    pub id: String,
    pub last_output_ms: u64,
    /// 最后一次用户写入（区分回显与响应）
    pub last_write_ms: u64,
    /// 是否存在存活 <60s 的子进程（工具调用特征；MCP 等常驻子进程不计）
    pub has_children: bool,
    /// 工作目录（用于"同目录存活新会话 PTY"的附着守卫）
    pub cwd: String,
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[derive(Default)]
pub struct PtyMap(
    pub Mutex<HashMap<String, PtyHandle>>,
    Mutex<VecDeque<(String, PtySnapshot)>>,
);

impl PtyMap {
    fn remember_finished(&self, id: &str, mut snapshot: PtySnapshot) {
        snapshot.exited = true;
        let mut finished = self.1.lock().unwrap();
        finished.retain(|(key, _)| key != id);
        if finished.len() >= FINISHED_SNAPSHOT_CAP {
            finished.pop_front();
        }
        finished.push_back((id.to_string(), snapshot));
    }
}

#[derive(Serialize, Clone)]
struct PtyEvent {
    id: String,
    /// 本块首字节在 PTY 输出流中的绝对偏移，用于前端与快照去重。
    offset: u64,
    data: Vec<u8>,
}

#[derive(Default)]
struct OutputBuffer {
    data: Vec<u8>,
    /// 累计写入字节数；即下一块输出的起始偏移。
    end_offset: u64,
}

impl OutputBuffer {
    fn append(&mut self, data: &[u8]) -> u64 {
        let offset = self.end_offset;
        self.end_offset += data.len() as u64;
        self.data.extend_from_slice(data);
        if self.data.len() > BUFFER_CAP {
            let drop = self.data.len() - BUFFER_CAP;
            self.data.drain(..drop);
        }
        offset
    }

    fn snapshot(&self) -> PtySnapshot {
        PtySnapshot {
            offset: self.end_offset - self.data.len() as u64,
            data: self.data.clone(),
            exited: false,
        }
    }
}

#[derive(Serialize, Clone)]
pub struct PtySnapshot {
    pub offset: u64,
    pub data: Vec<u8>,
    pub exited: bool,
}

/// 剥离宿主进程注入的 Claude/Codex 运行标记，并补齐 TUI 需要的颜色变量。
/// 不剥离的话，从 Claude Code 会话里启动的 ReSession 会把标记传给 PTY 里的
/// claude，被当作 child session **关闭转录保存**（jsonl 不再追加，会话丢失）。
fn build_command(spec: &ResumeSpec) -> CommandBuilder {
    let mut cmd = CommandBuilder::new(&spec.program);
    cmd.args(&spec.args);
    cmd.cwd(&spec.cwd);
    cmd.env_clear();
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
    if let Some(shell_path) = session_core::platform::login_shell_path() {
        let merged = match &app_path {
            Some(p) if !p.is_empty() => format!("{}:{}", shell_path, p),
            _ => shell_path,
        };
        cmd.env("PATH", merged);
    }
    // 颜色三件套：TERM/COLORTERM 是常规声明；FORCE_COLOR=3 强制 Node/chalk
    // 走真彩，绕过其在 ConPTY 下不可靠的终端能力自动探测
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    cmd.env("FORCE_COLOR", "3");
    cmd
}

/// 幂等 spawn：该会话已有 PTY 时直接返回（调用方负责先行检查，这里兜底）。
pub fn spawn(
    app: &AppHandle,
    map: &PtyMap,
    id: String,
    spec: ResumeSpec,
    rows: u16,
    cols: u16,
) -> Result<(), String> {
    if map.0.lock().unwrap().contains_key(&id) {
        return Ok(());
    }
    map.1.lock().unwrap().retain(|(key, _)| key != &id);

    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| e.to_string())?;

    let child = pair
        .slave
        .spawn_command(build_command(&spec))
        .map_err(|e| e.to_string())?;
    let child = Arc::new(Mutex::new(child));
    // 父进程若持有 slave 会导致读端行为异常，必须立刻释放
    drop(pair.slave);

    let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
    let writer = pair.master.take_writer().map_err(|e| e.to_string())?;
    let buffer: Arc<Mutex<OutputBuffer>> = Arc::new(Mutex::new(OutputBuffer::default()));
    let last_output = Arc::new(Mutex::new(now_ms()));
    // 0 = 尚未写入：任何真实输出都晚于它
    let last_write = Arc::new(Mutex::new(0u64));
    let cwd = spec.cwd.display().to_string();

    map.0.lock().unwrap().insert(
        id.clone(),
        PtyHandle {
            writer,
            master: pair.master,
            child: child.clone(),
            buffer: Arc::clone(&buffer),
            last_output: Arc::clone(&last_output),
            last_write: Arc::clone(&last_write),
            cwd,
        },
    );

    // 读线程：pty 原始字节 → 前端事件 + 回放缓冲。不解析、不修改内容。
    let app = app.clone();
    let pty_id = id.clone();
    log::info!("pty spawned: {} -> {}", id, spec.command_line());
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let offset = buffer.lock().unwrap().append(&buf[..n]);
                    *last_output.lock().unwrap() = now_ms();
                    if app
                        .emit(
                            "pty-out",
                            PtyEvent {
                                id: pty_id.clone(),
                                offset,
                                data: buf[..n].to_vec(),
                            },
                        )
                        .is_err()
                    {
                        break; // 窗口已关闭
                    }
                }
            }
        }
        let _ = child.lock().unwrap().wait();
        // 先保留有限数量的最终输出快照：CLI 若在前端挂载前迅速退出，
        // pty_snapshot 仍可找回真实错误，而不是只得到 "pty not found"。
        let ptys = app.state::<PtyMap>();
        ptys.remember_finished(&pty_id, buffer.lock().unwrap().snapshot());
        // 自然退出也要把句柄从运行列表移除（此前只有 close() 会删，
        // 退出的 PTY 会永远残留在 pty_list 里）。先删再广播，
        // 前端收到 pty-exit 刷新时列表已经干净。
        // 锁序说明：此处已释放 child 锁再取 map 锁，与 close() 的
        // map→child 顺序不会形成环路。
        if let Some(h) = ptys.0.lock().unwrap().remove(&pty_id) {
            drop(h); // master/缓冲随句柄释放，读端 EOF 生效
        }
        log::info!("pty exited: {}", pty_id);
        let _ = app.emit(
            "pty-exit",
            PtyEvent {
                id: pty_id,
                offset: 0,
                data: Vec::new(),
            },
        );
    });
    Ok(())
}

pub fn write(map: &PtyMap, id: &str, data: &str) -> Result<(), String> {
    let mut m = map.0.lock().unwrap();
    let h = m.get_mut(id).ok_or_else(|| "pty not found".to_string())?;
    h.writer.write_all(data.as_bytes()).map_err(|e| e.to_string())?;
    *h.last_write.lock().unwrap() = now_ms();
    Ok(())
}

pub fn resize(map: &PtyMap, id: &str, rows: u16, cols: u16) -> Result<(), String> {
    let m = map.0.lock().unwrap();
    let h = m.get(id).ok_or_else(|| "pty not found".to_string())?;
    h.master
        .resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| e.to_string())
}

/// 回放缓冲快照：原始字节与起始偏移，用于和实时事件去重。
pub fn snapshot(map: &PtyMap, id: &str) -> Result<PtySnapshot, String> {
    let current = {
        let running = map.0.lock().unwrap();
        running
            .get(id)
            .map(|h| h.buffer.lock().unwrap().snapshot())
    };
    if let Some(current) = current {
        return Ok(current);
    }
    map.1
        .lock()
        .unwrap()
        .iter()
        .rev()
        .find(|(key, _)| key == id)
        .map(|(_, snapshot)| snapshot.clone())
        .ok_or_else(|| "pty not found".to_string())
}

/// 仍存活的 PTY 状态（含最后输出时间/工作目录，供忙闲感知与附着守卫）
pub fn list(map: &PtyMap) -> Vec<PtyStatus> {
    map.0.lock()
        .unwrap()
        .iter()
        .map(|(id, h)| PtyStatus {
            id: id.clone(),
            last_output_ms: *h.last_output.lock().unwrap(),
            last_write_ms: *h.last_write.lock().unwrap(),
            has_children: has_young_child(h.child.lock().unwrap().process_id()),
            cwd: h.cwd.clone(),
        })
        .collect()
}

/// 是否存在存活 <60s 的直接子进程——claude/codex 执行工具时派生的
/// 子进程是年轻的；MCP server 等随会话常驻的子进程是老的，不构成
/// "执行中"信号（否则空闲时永远误报忙）。
fn has_young_child(pid: Option<u32>) -> bool {
    let Some(pid) = pid else { return false };
    // Windows 子进程枚举成本高，退回纯输出阈值判定
    if cfg!(windows) {
        return false;
    }
    let Ok(out) = std::process::Command::new("pgrep")
        .arg("-P")
        .arg(pid.to_string())
        .output()
    else {
        return false;
    };
    if !out.status.success() || out.stdout.is_empty() {
        return false;
    }
    // 限制检查数量：子进程多时（管道树）逐个 ps 太浪费
    String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .take(8)
        .any(|child_pid| young_process(child_pid))
}

fn young_process(pid: &str) -> bool {
    // etime（非 etimes）：macOS 与 Linux 都支持，格式 [[dd-]hh:]mm:ss
    let Ok(out) = std::process::Command::new("ps")
        .arg("-o")
        .arg("etime=")
        .arg("-p")
        .arg(pid)
        .output()
    else {
        return false;
    };
    if !out.status.success() {
        return false;
    }
    etime_seconds(&String::from_utf8_lossy(&out.stdout))
        .map(|secs| secs < 60)
        .unwrap_or(false)
}

/// 解析 `ps -o etime=` 输出为秒（跨 macOS/Linux 格式差异）
fn etime_seconds(raw: &str) -> Option<u64> {
    let s = raw.trim();
    let (days, rest) = match s.split_once('-') {
        Some((d, r)) => (d.parse::<u64>().ok()?, r),
        None => (0, s),
    };
    let parts: Vec<Option<u64>> = rest
        .split(':')
        .map(|p| p.trim().parse().ok())
        .collect();
    match parts.as_slice() {
        [Some(m), Some(sec)] => Some(days * 86400 + m * 60 + sec),
        [Some(h), Some(m), Some(sec)] => Some(days * 86400 + h * 3600 + m * 60 + sec),
        _ => None,
    }
}

/// 目录比较的归一化：Windows 与 macOS（APFS/HFS+ 默认不区分大小写）
/// 忽略大小写与分隔符；Linux 保留大小写语义。
/// （与前端 paths.ts 的 normalizePath 语义一致）
fn normalize_dir(p: &str) -> String {
    let trimmed = p.trim_end_matches(['\\', '/']).replace('\\', "/");
    let is_win = trimmed.len() >= 2 && trimmed.as_bytes()[1] == b':';
    if is_win || cfg!(target_os = "macos") {
        trimmed.to_lowercase()
    } else {
        trimmed
    }
}

/// 是否存在指定目录、指定 Provider 下存活的"新会话"合成 PTY——它可能正在写该目录的
/// jsonl，此时删除/移动该会话文件属于未定义行为（删除守卫用）。
pub fn has_live_new_at(map: &PtyMap, cwd: &str, provider: &str) -> bool {
    let dir = normalize_dir(cwd);
    if dir.is_empty() {
        return false;
    }
    let prefix = format!("new:{provider}:");
    map.0.lock().unwrap().iter().any(|(id, h)| {
        id.starts_with(&prefix) && normalize_dir(&h.cwd) == dir
    })
}

pub fn close(map: &PtyMap, id: &str) -> Result<(), String> {
    let mut m = map.0.lock().unwrap();
    if let Some(h) = m.remove(id) {
        let _ = h.child.lock().unwrap().kill();
        // master/buffer 随 handle drop
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_buffer_offsets_survive_truncation() {
        let mut buffer = OutputBuffer::default();
        assert_eq!(buffer.append(b"first"), 0);
        assert_eq!(buffer.append(b"second"), 5);
        let snapshot = buffer.snapshot();
        assert_eq!(snapshot.offset, 0);
        assert_eq!(snapshot.data.as_slice(), &b"firstsecond"[..]);

        let oversized = vec![b'x'; BUFFER_CAP + 4];
        assert_eq!(buffer.append(&oversized), 11);
        let snapshot = buffer.snapshot();
        assert_eq!(snapshot.offset, 15);
        assert_eq!(snapshot.data.len(), BUFFER_CAP);
        assert!(snapshot.data.iter().all(|byte| *byte == b'x'));
        assert_eq!(buffer.append(b"end"), (BUFFER_CAP + 15) as u64);
        let snapshot = buffer.snapshot();
        assert_eq!(snapshot.offset, 18);
        assert!(snapshot.data.ends_with(b"end"));
    }

    #[test]
    fn finished_snapshot_preserves_fast_exit_output_and_is_bounded() {
        let map = PtyMap::default();
        let mut buffer = OutputBuffer::default();
        buffer.append(b"configuration error\r\n");
        map.remember_finished("quick", buffer.snapshot());
        let result = snapshot(&map, "quick").unwrap();
        assert_eq!(result.data.as_slice(), &b"configuration error\r\n"[..]);
        assert!(result.exited);

        for index in 0..FINISHED_SNAPSHOT_CAP {
            map.remember_finished(&format!("later-{index}"), buffer.snapshot());
        }
        assert!(snapshot(&map, "quick").is_err());
        assert!(snapshot(&map, "later-0").unwrap().exited);
    }

    #[test]
    fn normalize_dir_matches_paths_ts_semantics() {
        // Windows：分隔符与大小写不敏感
        assert_eq!(normalize_dir("C:\\a\\b\\"), normalize_dir("c:/a/b"));
        // Linux：大小写敏感；macOS 文件系统大小写不敏感，比较归一
        if !cfg!(target_os = "macos") {
            assert_ne!(normalize_dir("/Foo"), normalize_dir("/foo"));
        } else {
            assert_eq!(normalize_dir("/Foo"), normalize_dir("/foo"));
        }
        assert_eq!(normalize_dir("/Foo/"), normalize_dir("/Foo"));
    }

    /// 契约：PtyStatus 必须 camelCase（曾因漏 rename 导致忙闲检测全挂）
    #[test]
    fn pty_status_keys_are_camel_case() {
        let s = PtyStatus {
            id: "x".into(),
            last_output_ms: 7,
            last_write_ms: 3,
            has_children: true,
            cwd: "C:\\".into(),
        };
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["lastOutputMs"], 7);
        assert_eq!(v["lastWriteMs"], 3);
        assert_eq!(v["hasChildren"], true);
        assert!(v.get("last_output_ms").is_none());
        for k in v.as_object().unwrap().keys() {
            assert!(!k.contains('_'), "DTO key `{k}` 含蛇形命名");
        }
    }

    #[test]
    fn etime_seconds_parses_cross_platform_formats() {
        assert_eq!(etime_seconds("42"), Some(42));
        assert_eq!(etime_seconds(" 03:05 "), Some(185));
        assert_eq!(etime_seconds("01:02:03"), Some(3723));
        assert_eq!(etime_seconds("2-03:04:05"), Some(2 * 86400 + 3 * 3600 + 4 * 60 + 5));
        assert_eq!(etime_seconds("junk"), None);
    }
}
