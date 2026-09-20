mod pty;
mod settings;

use std::collections::HashMap;
use std::path::PathBuf;

use session_core::{registry, Event, SearchHit, SessionMeta, SessionProvider};
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

#[tauri::command]
fn get_settings(settings: State<SettingsState>) -> Settings {
    settings.0.lock().unwrap().clone()
}

/// 保存设置；claude 路径覆盖立即生效（影响后续 spawn），忙闲阈值由前端读取
#[tauri::command]
fn save_settings(
    settings: State<SettingsState>,
    claude_path: Option<String>,
    busy_ms: u64,
) -> Result<Settings, String> {
    let mut s = settings.0.lock().unwrap();
    s.claude_path = claude_path
        .as_deref()
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(str::to_string);
    s.busy_ms = busy_ms.clamp(1000, 30000);
    s.save()?;
    session_core::set_claude_binary_override(s.claude_path.clone());
    Ok(s.clone())
}

/// 删除单个别名（key 形如 "claude:<uuid>"）
#[tauri::command]
fn remove_alias(settings: State<SettingsState>, key: String) -> Result<(), String> {
    let mut s = settings.0.lock().unwrap();
    s.aliases.remove(&key);
    s.save()
}

/// 归档/取消归档会话（keys 形如 "claude:<uuid>"，支持批量）。
/// 只动 ReSession 配置层：原生 JSONL 保留，列表默认隐藏。
#[tauri::command]
fn set_archived(
    settings: State<SettingsState>,
    keys: Vec<String>,
    archived: bool,
) -> Result<(), String> {
    if keys.is_empty() {
        return Ok(());
    }
    let mut s = settings.0.lock().unwrap();
    for key in &keys {
        if archived {
            if !s.archived.contains(key) {
                s.archived.push(key.clone());
            }
        } else {
            s.archived.retain(|k| k != key);
        }
    }
    s.save()
}

/// 删除会话：JSONL 移入系统废纸篓（可恢复，与手动删除等价），并清理
/// 别名/归档残留。守卫：该会话有存活 PTY 时拒绝——正在被写入的文件不能动。
#[tauri::command]
fn delete_session(
    settings: State<SettingsState>,
    ptys: State<PtyMap>,
    meta: SessionMeta,
) -> Result<(), String> {
    // 注意锁的粒度：contains_key 检查与 has_live_new_at 各自短暂拿锁，
    // 不能在持有 map 锁的情况下调用会再次拿锁的辅助函数（Mutex 不可重入 = 死锁）
    if ptys.0.lock().unwrap().contains_key(&meta.id) {
        return Err("会话正在运行，请先关闭终端再删除".into());
    }
    // "新会话"合成 PTY 的键不是会话 id，但它的 claude 正在写这个会话的
    // jsonl（P0 守卫的同类场景）——按 cwd 匹配拦截
    if let Some(cwd) = meta.cwd.as_deref() {
        if pty::has_live_new_at(&ptys, cwd) {
            return Err("该会话正由\"新会话\"终端创建中，请先关闭那个终端再删除".into());
        }
    }
    let path = PathBuf::from(&meta.source_file);
    if path.exists() {
        trash::delete(&path).map_err(|e| format!("移入废纸篓失败: {e}"))?;
    }
    let key = format!("{}:{}", meta.provider, meta.id);
    let mut s = settings.0.lock().unwrap();
    s.aliases.remove(&key);
    s.archived.retain(|k| k != &key);
    s.save()
}

/// ReSession 配置文件路径（可能尚不存在——首次写入时才落盘）
#[tauri::command]
fn settings_path() -> Result<String, String> {
    settings::Settings::path()
        .map(|p| p.display().to_string())
        .ok_or_else(|| "no home directory".into())
}

/// 在系统文件管理器中定位 settings.json（不存在则先创建空文件）
#[tauri::command]
fn reveal_settings_file() -> Result<String, String> {
    let Some(p) = settings::Settings::path() else {
        return Err("no home directory".into());
    };
    if !p.exists() {
        if let Some(dir) = p.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        std::fs::write(&p, b"{}").map_err(|e| e.to_string())?;
    }
    #[cfg(windows)]
    {
        std::process::Command::new("explorer")
            .arg(format!("/select,{}", p.display()))
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg("-R")
            .arg(&p)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "linux")]
    {
        let dir = p.parent().map(|d| d.to_path_buf()).unwrap_or_default();
        std::process::Command::new("xdg-open")
            .arg(dir)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    Ok(p.display().to_string())
}

/// 全文搜索对话正文（v1 设计见 architecture.md 3.2b）。
/// 结果按会话最近活跃排序，截断 50 条；标题套用别名。
#[tauri::command]
fn search_sessions(
    query: String,
    settings: State<SettingsState>,
) -> Result<Vec<SearchHit>, String> {
    let q = query.trim().to_string();
    if q.is_empty() {
        return Ok(vec![]);
    }
    let aliases = settings.0.lock().unwrap().aliases.clone();
    let mut hits = Vec::new();
    for p in registry() {
        match p.search(&q) {
            Ok(mut h) => hits.append(&mut h),
            Err(e) => eprintln!("[{}] search failed: {e}", p.name()),
        }
    }
    for h in &mut hits {
        let key = format!("{}:{}", h.session.provider, h.session.id);
        if let Some(alias) = aliases.get(&key) {
            h.session.title = Some(alias.clone());
        }
    }
    hits.sort_by(|a, b| {
        b.session
            .modified_at
            .cmp(&a.session.modified_at)
            .then(a.session.id.cmp(&b.session.id))
            .then(a.event_index.cmp(&b.event_index))
    });
    hits.truncate(50);
    Ok(hits)
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

#[cfg(test)]
mod tests {
    use super::KnownProject;

    /// 契约：KnownProject 必须 camelCase
    #[test]
    fn known_project_keys_are_camel_case() {
        let k = KnownProject {
            path: "C:\\x".into(),
            last_active: Some("2026-09-19T00:00:00Z".into()),
        };
        let v = serde_json::to_value(&k).unwrap();
        assert!(v.get("lastActive").is_some());
        assert!(v.get("last_active").is_none());
        for key in v.as_object().unwrap().keys() {
            assert!(!key.contains('_'), "DTO key `{key}` 含蛇形命名");
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let loaded = Settings::load();
    session_core::set_claude_binary_override(loaded.claude_path.clone());
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(PtyMap::default())
        .manage(SettingsState(std::sync::Mutex::new(loaded)))
        .invoke_handler(tauri::generate_handler![
            scan_sessions,
            load_transcript,
            rename_session,
            search_sessions,
            get_settings,
            save_settings,
            remove_alias,
            set_archived,
            delete_session,
            settings_path,
            reveal_settings_file,
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
