mod pty;
mod settings;

use std::collections::{HashMap, HashSet};
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
    let mut errors: Vec<String> = Vec::new();
    let mut ok_providers: HashSet<String> = HashSet::new();
    let mut prunable_providers: HashSet<String> = HashSet::new();
    // 单 provider 失败不拖垮整体（与 design.md 的宽容原则一致）；
    // 但全部失败且零结果时向上抛——静默空列表会让用户误以为没有会话
    for p in registry() {
        match p.scan() {
            Ok(mut s) => {
                ok_providers.insert(p.name().to_string());
                if p.can_prune_missing_metadata() {
                    prunable_providers.insert(p.name().to_string());
                }
                all.append(&mut s);
            }
            Err(e) => {
                log::warn!("[{}] scan failed: {e}", p.name());
                errors.push(format!("{} 扫描失败: {e}", p.name()));
            }
        }
    }
    if ok_providers.is_empty() && !errors.is_empty() {
        return Err(format!("扫描会话失败：{}", errors.join("；")));
    }
    prune_orphan_keys(&settings, &all, &prunable_providers);
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

/// 清洗孤儿 key：会话文件被外部（终端/文件管理器）删除后，settings.json 里的
/// 别名/归档键不再指向任何真实会话，随扫描顺带剔除。
/// 只动「本次扫描成功且缺失代表删除」的 provider 的 key。
/// Codex 原生归档会把文件移出活跃目录，不能据此删掉用户别名。
/// 洗不掉的极端情况（瞬时 IO 错误）代价只是用户重新设一次别名。
fn prune_orphan_keys(
    settings: &SettingsState,
    all: &[SessionMeta],
    ok_providers: &HashSet<String>,
) {
    if ok_providers.is_empty() {
        return;
    }
    let live: HashSet<String> = all
        .iter()
        .map(|m| format!("{}:{}", m.provider, m.id))
        .collect();
    let mut s = settings.0.lock().unwrap();
    let before = (s.archived.len(), s.aliases.len());
    s.archived.retain(|k| !is_orphan_key(k, &live, ok_providers));
    s.aliases.retain(|k, _| !is_orphan_key(k, &live, ok_providers));
    if (s.archived.len(), s.aliases.len()) != before {
        let _ = s.save(); // 清洗失败不阻塞扫描（下次扫描会再试）
    }
}

#[tauri::command]
async fn load_transcript(meta: SessionMeta) -> Result<Vec<Event>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        provider_for(&meta.provider)?
            .load_transcript(&meta)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
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

/// 保存设置；Agent 路径覆盖立即生效（影响后续 spawn），忙闲阈值由前端读取
#[tauri::command]
fn save_settings(
    settings: State<SettingsState>,
    claude_path: Option<String>,
    codex_path: Option<String>,
    busy_ms: u64,
) -> Result<Settings, String> {
    let mut s = settings.0.lock().unwrap();
    s.claude_path = claude_path
        .as_deref()
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(str::to_string);
    s.codex_path = codex_path
        .as_deref()
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(str::to_string);
    s.busy_ms = busy_ms.clamp(1000, 30000);
    s.save()?;
    session_core::set_claude_binary_override(s.claude_path.clone());
    session_core::set_codex_binary_override(s.codex_path.clone());
    Ok(s.clone())
}

/// 删除单个别名（key 形如 "provider:<uuid>"）
#[tauri::command]
fn remove_alias(settings: State<SettingsState>, key: String) -> Result<(), String> {
    let mut s = settings.0.lock().unwrap();
    s.aliases.remove(&key);
    s.save()
}

/// 归档/取消归档会话（keys 形如 "provider:<uuid>"，支持批量）。
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
    if !provider_for(&meta.provider)?.can_trash_native() {
        return Err("Codex 原生会话请在 Codex 中删除；ReSession 暂不移动其索引文件".into());
    }
    // 注意锁的粒度：contains_key 检查与 has_live_new_at 各自短暂拿锁，
    // 不能在持有 map 锁的情况下调用会再次拿锁的辅助函数（Mutex 不可重入 = 死锁）
    if ptys.0.lock().unwrap().contains_key(&format!("{}:{}", meta.provider, meta.id)) {
        return Err("会话正在运行，请先关闭终端再删除".into());
    }
    // "新会话"合成 PTY 的键不是会话 id，但它的 claude 正在写这个会话的
    // jsonl（P0 守卫的同类场景）——按 cwd 匹配拦截
    if let Some(cwd) = meta.cwd.as_deref() {
        if pty::has_live_new_at(&ptys, cwd, &meta.provider) {
            return Err("该会话正由\"新会话\"终端创建中，请先关闭那个终端再删除".into());
        }
    }
    let path = PathBuf::from(&meta.source_file);
    if path.exists() {
        trash::delete(&path).map_err(|e| format!("移入废纸篓失败: {e}"))?;
        log::info!("session trashed: {} ({})", meta.id, path.display());
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

/// 日志目录路径
#[tauri::command]
fn logs_dir() -> Result<String, String> {
    settings::Settings::logs_dir()
        .map(|p| p.display().to_string())
        .ok_or_else(|| "no home directory".into())
}

/// 在系统文件管理器中打开日志目录（不存在则先创建）
#[tauri::command]
fn reveal_logs_dir() -> Result<String, String> {
    let Some(dir) = settings::Settings::logs_dir() else {
        return Err("no home directory".into());
    };
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    #[cfg(windows)]
    {
        std::process::Command::new("explorer")
            .arg(dir.display().to_string())
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&dir)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(&dir)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    Ok(dir.display().to_string())
}

/// 用系统默认浏览器打开 https 链接（严格白名单，防参数注入）
#[tauri::command]
fn open_url(url: String) -> Result<(), String> {
    if !url.starts_with("https://") {
        return Err("only https urls are allowed".into());
    }
    #[cfg(windows)]
    {
        std::process::Command::new("explorer")
            .arg(&url)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&url)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(&url)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    Ok(())
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
async fn search_sessions(
    query: String,
    settings: State<'_, SettingsState>,
) -> Result<Vec<SearchHit>, String> {
    let q = query.trim().to_string();
    if q.is_empty() {
        return Ok(vec![]);
    }
    let aliases = settings.0.lock().unwrap().aliases.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut hits = Vec::new();
        let mut ok_providers = 0usize;
        let mut errors = Vec::new();
        for p in registry() {
            match p.search(&q) {
                Ok(mut h) => {
                    ok_providers += 1;
                    hits.append(&mut h);
                }
                Err(e) => {
                    log::warn!("[{}] search failed: {e}", p.name());
                    errors.push(format!("{} 搜索失败: {e}", p.name()));
                }
            }
        }
        if ok_providers == 0 && !errors.is_empty() {
            return Err(errors.join("；"));
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
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 为会话打开原生 PTY 并运行 provider 给出的 resume 命令，返回 PTY 会话 id。
/// 幂等：该会话已有存活的 PTY 时直接返回现有 id（切换会话回来 = 重新附着）。
#[tauri::command]
fn resume_session(
    app: AppHandle,
    ptys: State<PtyMap>,
    meta: SessionMeta,
) -> Result<String, String> {
    let pty_id = format!("{}:{}", meta.provider, meta.id);
    if ptys.0.lock().unwrap().contains_key(&pty_id) {
        return Ok(pty_id);
    }
    let spec = provider_for(&meta.provider)?
        .resume_command(&meta)
        .map_err(|e| e.to_string())?;
    // 初始 24x80，前端 xterm 挂载后立即上报真实尺寸
    pty::spawn(&app, &ptys, pty_id.clone(), spec, 24, 80)?;
    Ok(pty_id)
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
    let mut ok_providers = 0usize;
    let mut errors = Vec::new();
    for p in registry() {
        match p.scan() {
            Ok(mut sessions) => {
                ok_providers += 1;
                all.append(&mut sessions);
            }
            Err(error) => errors.push(format!("{} 扫描失败: {error}", p.name())),
        }
    }
    if ok_providers == 0 && !errors.is_empty() {
        return Err(errors.join("；"));
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

/// 在指定目录启动指定原生 Agent；PTY 用合成键 `new:<provider>:<uuid>`；
/// Agent 写入 jsonl 后，下次扫描即出现在会话列表。
#[tauri::command]
fn new_session(
    app: AppHandle,
    ptys: State<PtyMap>,
    cwd: String,
    provider: String,
) -> Result<String, String> {
    let path = PathBuf::from(&cwd);
    if !path.is_dir() {
        return Err(format!("目录不存在: {cwd}"));
    }
    let spec = provider_for(&provider)?
        .new_session_command(path)
        .map_err(|e| e.to_string())?;
    let id = format!("new:{provider}:{}", uuid::Uuid::new_v4());
    pty::spawn(&app, &ptys, id.clone(), spec, 24, 80)?;
    Ok(id)
}

/// 孤儿判定：key 的 provider 本次扫描成功、但会话不在扫描结果中。
/// 格式不明的旧键一律保留（保守，不误删）。
fn is_orphan_key(key: &str, live: &HashSet<String>, ok_providers: &HashSet<String>) -> bool {
    match key.split_once(':') {
        None => false,
        Some((prov, _)) => ok_providers.contains(prov) && !live.contains(key),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orphan_key_rules() {
        let live: HashSet<String> = ["claude:aaa", "codex:bbb"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let ok: HashSet<String> = ["claude".to_string(), "codex".to_string()].into_iter().collect();
        // provider 扫描成功 + 会话消失 = 孤儿
        assert!(is_orphan_key("claude:gone", &live, &ok));
        // 还在的会话不是孤儿
        assert!(!is_orphan_key("claude:aaa", &live, &ok));
        // 扫描失败的 provider 数据未卜，不碰
        let only_claude: HashSet<String> = ["claude".to_string()].into_iter().collect();
        assert!(!is_orphan_key("codex:missing", &live, &only_claude));
        // 格式不明的旧键保留
        assert!(!is_orphan_key("no-colon-here", &live, &ok));
    }

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
    session_core::set_codex_binary_override(loaded.codex_path.clone());
    let log_dir = Settings::logs_dir();
    if let Some(dir) = &log_dir {
        let _ = std::fs::create_dir_all(dir);
    }
    let mut log_targets = vec![tauri_plugin_log::Target::new(
        tauri_plugin_log::TargetKind::Stdout,
    )];
    if let Some(path) = log_dir {
        log_targets.push(tauri_plugin_log::Target::new(
            tauri_plugin_log::TargetKind::Folder {
                path,
                file_name: Some("resession".into()),
            },
        ));
    }
    log_targets.push(tauri_plugin_log::Target::new(
        tauri_plugin_log::TargetKind::Webview,
    ));
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(
            tauri_plugin_log::Builder::new()
                .targets(log_targets)
                .level(log::LevelFilter::Info)
                .max_file_size(2_000_000)
                .rotation_strategy(tauri_plugin_log::RotationStrategy::KeepAll)
                .build(),
        )
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
            logs_dir,
            reveal_logs_dir,
            open_url,
            resume_session,
            new_session,
            known_projects,
            pty_write,
            pty_resize,
            pty_snapshot,
            pty_list,
            pty_close
        ])
        .setup(|app| {
            log::info!("ReSession v{} started", app.package_info().version);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
