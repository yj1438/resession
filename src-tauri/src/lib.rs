mod pty;
mod settings;

use std::collections::HashMap;
use std::path::PathBuf;

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

/// 已知项目列表（从会话 cwd 去重，按最近活跃倒序）——"新会话"面板的数据源
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct KnownProject {
    path: String,
    last_active: Option<String>,
}

#[tauri::command]
fn known_projects() -> Result<Vec<KnownProject>, String> {
    let mut all = Vec::new();
    for p in registry() {
        if let Ok(mut s) = p.scan() {
            all.append(&mut s);
        }
    }
    // cwd -> 最近 modified_at（ISO 字符串可直接比较）
    let mut latest: HashMap<String, String> = HashMap::new();
    for m in all {
        let Some(cwd) = m.cwd else { continue };
        let entry = latest.entry(cwd).or_default();
        if m.modified_at.as_deref().unwrap_or("") > entry.as_str() {
            *entry = m.modified_at.unwrap_or_default();
        }
    }
    let mut projects: Vec<KnownProject> = latest
        .into_iter()
        .map(|(path, last_active)| KnownProject {
            path,
            last_active: Some(last_active).filter(|s| !s.is_empty()),
        })
        .collect();
    projects.sort_by(|a, b| b.last_active.cmp(&a.last_active));
    Ok(projects)
}

/// 在指定目录启动全新原生会话（裸 `claude`），PTY 用合成键 `new:<uuid>`；
/// claude 启动后会写入 jsonl，下次扫描即出现在会话列表。
#[tauri::command]
fn new_session(
    app: AppHandle,
    ptys: State<PtyMap>,
    cwd: String,
) -> Result<String, String> {
    let path = PathBuf::from(&cwd);
    if !path.is_dir() {
        return Err(format!("目录不存在: {cwd}"));
    }
    let spec = provider_for("claude")?
        .new_session_command(path)
        .map_err(|e| e.to_string())?;
    let id = format!("new:{}", uuid::Uuid::new_v4());
    pty::spawn(&app, &ptys, id.clone(), spec, 24, 80)?;
    Ok(id)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(PtyMap::default())
        .manage(SettingsState(std::sync::Mutex::new(Settings::load())))
        .invoke_handler(tauri::generate_handler![
            scan_sessions,
            load_transcript,
            rename_session,
            resume_session,
            new_session,
            known_projects,
            pty_write,
            pty_resize,
            pty_snapshot,
            pty_list,
            pty_close
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
