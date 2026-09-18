mod pty;
mod settings;

use session_core::{registry, Event, SessionMeta, SessionProvider};
use tauri::{AppHandle, State};

use pty::PtyMap;
use settings::{Settings, SettingsState};

fn provider_for(name: &str) -> Result<Box<dyn SessionProvider>, String> {
    registry()
        .into_iter()
        .find(|p| p.name() == name)
        .ok_or_else(|| format!("unknown provider: {name}"))
}

#[tauri::command]
fn scan_sessions(settings: State<SettingsState>) -> Result<Vec<SessionMeta>, String> {
    let aliases = settings.0.lock().unwrap().aliases.clone();
    let mut all = Vec::new();
    // 单 provider 失败不拖垮整体（与 design.md 的宽容原则一致）
    for p in registry() {
        match p.scan() {
            Ok(mut s) => all.append(&mut s),
            Err(e) => eprintln!("[{}] scan failed: {e}", p.name()),
        }
    }
    // 别名覆盖：ReSession 别名 > 原生 /rename > summary > 首条消息
    for meta in &mut all {
        let key = format!("{}:{}", meta.provider, meta.id);
        if let Some(alias) = aliases.get(&key) {
            meta.title = Some(alias.clone());
        }
    }
    all.sort_by(|a, b| b.modified_at.cmp(&a.modified_at));
    Ok(all)
}

#[tauri::command]
fn load_transcript(meta: SessionMeta) -> Result<Vec<Event>, String> {
    provider_for(&meta.provider)?
        .load_transcript(&meta)
        .map_err(|e| e.to_string())
}

/// ReSession 别名（name 为空 = 清除别名，回落到原生标题）
#[tauri::command]
fn rename_session(
    settings: State<SettingsState>,
    provider: String,
    id: String,
    name: String,
) -> Result<(), String> {
    let mut s = settings.0.lock().unwrap();
    let key = format!("{provider}:{id}");
    let name = name.trim();
    if name.is_empty() {
        s.aliases.remove(&key);
    } else {
        s.aliases.insert(key, name.to_string());
    }
    s.save()
}

/// 为会话打开原生 PTY 并运行 provider 给出的 resume 命令，返回 PTY 会话 id。
/// 幂等：该会话已有存活的 PTY 时直接返回现有 id（切换会话回来 = 重新附着）。
#[tauri::command]
fn resume_session(
    app: AppHandle,
    ptys: State<PtyMap>,
    meta: SessionMeta,
) -> Result<String, String> {
    if ptys.0.lock().unwrap().contains_key(&meta.id) {
        return Ok(meta.id.clone());
    }
    let spec = provider_for(&meta.provider)?
        .resume_command(&meta)
        .map_err(|e| e.to_string())?;
    // 初始 24x80，前端 xterm 挂载后立即上报真实尺寸
    pty::spawn(&app, &ptys, meta.id.clone(), spec, 24, 80)?;
    Ok(meta.id)
}

#[tauri::command]
fn pty_write(ptys: State<PtyMap>, id: String, data: String) -> Result<(), String> {
    pty::write(&ptys, &id, &data)
}

#[tauri::command]
fn pty_resize(ptys: State<PtyMap>, id: String, rows: u16, cols: u16) -> Result<(), String> {
    pty::resize(&ptys, &id, rows, cols)
}

/// 回放缓冲（原始字节），用于切回会话时重建终端画面
#[tauri::command]
fn pty_snapshot(ptys: State<PtyMap>, id: String) -> Result<Vec<u8>, String> {
    pty::snapshot(&ptys, &id)
}

/// 当前存活的 PTY 状态列表（侧栏运行中/忙闲标识）
#[tauri::command]
fn pty_list(ptys: State<PtyMap>) -> Vec<pty::PtyStatus> {
    pty::list(&ptys)
}

#[tauri::command]
fn pty_close(ptys: State<PtyMap>, id: String) -> Result<(), String> {
    pty::close(&ptys, &id)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(PtyMap::default())
        .manage(SettingsState(std::sync::Mutex::new(Settings::load())))
        .invoke_handler(tauri::generate_handler![
            scan_sessions,
            load_transcript,
            rename_session,
            resume_session,
            pty_write,
            pty_resize,
            pty_snapshot,
            pty_list,
            pty_close
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
