//! PTY 会话管理：portable-pty 生命周期 + 前端事件桥。
//!
//! 原则（architecture.md 3.3）：**不解析、不记录、不干预终端内容**。

use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use session_core::ResumeSpec;

pub struct PtyHandle {
    writer: Box<dyn Write + Send>,
    master: Box<dyn MasterPty + Send>,
    child: Arc<Mutex<Box<dyn Child + Send + Sync>>>,
}

#[derive(Default)]
pub struct PtyMap(pub Mutex<HashMap<String, PtyHandle>>);

#[derive(Serialize, Clone)]
struct PtyEvent {
    id: String,
    data: String,
}

pub fn spawn(
    app: &AppHandle,
    map: &PtyMap,
    id: String,
    spec: ResumeSpec,
    rows: u16,
    cols: u16,
) -> Result<(), String> {
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| e.to_string())?;

    let mut cmd = CommandBuilder::new(&spec.program);
    cmd.args(&spec.args);
    cmd.cwd(&spec.cwd);
    let child = pair.slave.spawn_command(cmd).map_err(|e| e.to_string())?;
    let child = Arc::new(Mutex::new(child));
    // 父进程若持有 slave 会导致读端行为异常，必须立刻释放
    drop(pair.slave);

    let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
    let writer = pair.master.take_writer().map_err(|e| e.to_string())?;

    map.0.lock().unwrap().insert(
        id.clone(),
        PtyHandle {
            writer,
            master: pair.master,
            child: child.clone(),
        },
    );

    // 读线程：pty 输出 → 前端事件。内容原样转发（lossy UTF-8），M1 已知限制见 roadmap
    let app = app.clone();
    let pty_id = id.clone();
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let data = String::from_utf8_lossy(&buf[..n]).into_owned();
                    if app
                        .emit("pty-out", PtyEvent {
                            id: pty_id.clone(),
                            data,
                        })
                        .is_err()
                    {
                        break; // 窗口已关闭
                    }
                }
            }
        }
        let _ = child.lock().unwrap().wait();
        let _ = app.emit("pty-exit", PtyEvent {
            id: pty_id,
            data: String::new(),
        });
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

pub fn close(map: &PtyMap, id: &str) -> Result<(), String> {
    let mut m = map.0.lock().unwrap();
    if let Some(h) = m.remove(id) {
        let _ = h.child.lock().unwrap().kill();
        // master 随 handle drop 关闭
    }
    Ok(())
}
