//! PTY 会话管理：portable-pty 生命周期 + 前端事件桥。
//!
//! 原则（architecture.md 3.3）：不解析、不记录、不干预终端内容。
//! 生命周期：PTY 以「会话 id」为键**常驻**，直到 claude 退出或用户显式关闭；
//! UI 切换只断开观看，回来时回放缓冲区重新附着（支持并行多会话）。

use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use session_core::ResumeSpec;

/// 每个 PTY 保留的输出回放缓冲上限（字节）
const BUFFER_CAP: usize = 512 * 1024;

pub struct PtyHandle {
    writer: Box<dyn Write + Send>,
    master: Box<dyn MasterPty + Send>,
    child: Arc<Mutex<Box<dyn Child + Send + Sync>>>,
    /// 原始字节缓冲：前端切走再切回时回放（字节级，避免多字节字符跨块损坏）
    buffer: Arc<Mutex<Vec<u8>>>,
}

#[derive(Default)]
pub struct PtyMap(pub Mutex<HashMap<String, PtyHandle>>);

#[derive(Serialize, Clone)]
struct PtyEvent {
    id: String,
    data: Vec<u8>,
}

/// 剥离宿主进程注入的 Claude 相关环境标记，并补齐 TUI 需要的颜色变量。
/// 不剥离的话，从 Claude Code 会话里启动的 ReSession 会把标记传给 PTY 里的
/// claude，被当作 child session **关闭转录保存**（jsonl 不再追加，会话丢失）。
fn build_command(spec: &ResumeSpec) -> CommandBuilder {
    let mut cmd = CommandBuilder::new(&spec.program);
    cmd.args(&spec.args);
    cmd.cwd(&spec.cwd);
    cmd.env_clear();
    for (k, v) in std::env::vars_os() {
        let name = k.to_string_lossy().to_string();
        let upper = name.to_uppercase();
        // CLAUDE*：child-session 标记会关闭转录保存；
        // CI / NO_COLOR：两者都会压掉 TUI 颜色
        if upper.starts_with("CLAUDE") || upper == "CI" || upper == "NO_COLOR" {
            continue;
        }
        cmd.env(&name, v);
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
    let buffer: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));

    map.0.lock().unwrap().insert(
        id.clone(),
        PtyHandle {
            writer,
            master: pair.master,
            child: child.clone(),
            buffer: Arc::clone(&buffer),
        },
    );

    // 读线程：pty 原始字节 → 前端事件 + 回放缓冲。不解析、不修改内容。
    let app = app.clone();
    let pty_id = id.clone();
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    {
                        let mut b = buffer.lock().unwrap();
                        if b.len() + n > BUFFER_CAP {
                            let drop = (b.len() + n - BUFFER_CAP).min(b.len());
                            b.drain(..drop);
                        }
                        b.extend_from_slice(&buf[..n]);
                    }
                    if app
                        .emit(
                            "pty-out",
                            PtyEvent {
                                id: pty_id.clone(),
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
        let _ = app.emit(
            "pty-exit",
            PtyEvent {
                id: pty_id,
                data: Vec::new(),
            },
        );
    });
    Ok(())
}

pub fn write(map: &PtyMap, id: &str, data: &str) -> Result<(), String> {
    let mut m = map.0.lock().unwrap();
    let h = m.get_mut(id).ok_or_else(|| "pty not found".to_string())?;
    h.writer.write_all(data.as_bytes()).map_err(|e| e.to_string())
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

/// 回放缓冲快照（原始字节，前端以 Uint8Array 写回 xterm）
pub fn snapshot(map: &PtyMap, id: &str) -> Result<Vec<u8>, String> {
    let m = map.0.lock().unwrap();
    let h = m.get(id).ok_or_else(|| "pty not found".to_string())?;
    Ok(h.buffer.lock().unwrap().clone())
}

/// 仍存活（运行中）的 PTY 会话 id 列表
pub fn list(map: &PtyMap) -> Vec<String> {
    map.0.lock().unwrap().keys().cloned().collect()
}

pub fn close(map: &PtyMap, id: &str) -> Result<(), String> {
    let mut m = map.0.lock().unwrap();
    if let Some(h) = m.remove(id) {
        let _ = h.child.lock().unwrap().kill();
        // master/buffer 随 handle drop
    }
    Ok(())
}
