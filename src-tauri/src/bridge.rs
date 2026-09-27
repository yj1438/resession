//! Agent Bridge：进程内 loopback HTTP 服务。
//!
//! skill（触发词 `trs`）通过它把一个 agent 会话的内容投递给另一个会话，
//! 并回读目标会话的原生 jsonl 增量。协议与不变量见 docs/bridge-design.md；
//! 传输层移植自 plugins/tmux-agent（send-keys→pty_write、capture-pane→jsonl tail）。
//!
//! 安全模型：仅绑定 127.0.0.1 + Bearer token（`~/.resession/bridge.json`，
//! 0600）。与 Docker socket 同级的"同机同用户互信"假设，文档已明示。

use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};
use tiny_http::{Header, Method, Request, Response, Server};

use session_core::ir::{Block, Role, SessionMeta};

use crate::pty::{self, PtyMap};
use crate::settings::SettingsState;

/// 一次可回读的转录消息（纯数据，便于单测过滤逻辑）
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TailMessage {
    pub index: usize,
    pub role: &'static str,
    pub text: String,
    pub timestamp: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")] // 跨端 DTO 契约（同 SessionMeta/PtyStatus，勿漏）
struct BridgeSession {
    id: String,
    provider: String,
    title: String,
    cwd: String,
    running: bool,
    busy: bool,
}

#[derive(Deserialize)]
struct SendBody {
    #[serde(rename = "targetId")]
    target_id: String,
    content: String,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// 进程生命周期内启动一次；阻塞线程随 app 退出终结
pub fn start(app: AppHandle) {
    std::thread::spawn(move || run_server(app));
}

fn run_server(app: AppHandle) {
    let server = match Server::http("127.0.0.1:0") {
        Ok(s) => s,
        Err(e) => {
            log::error!("[bridge] listen failed: {e}");
            return;
        }
    };
    let Some(addr) = server.server_addr().to_ip() else {
        log::error!("[bridge] cannot resolve listen address");
        return;
    };
    let token = uuid::Uuid::new_v4().to_string();
    if let Err(e) = write_discovery(addr.port(), &token) {
        log::error!("[bridge] discovery write failed: {e}");
        return;
    }
    log::info!("[bridge] listening on 127.0.0.1:{}", addr.port());
    for request in server.incoming_requests() {
        if let Err(e) = handle(&app, &token, request) {
            log::warn!("[bridge] response failed: {e}");
        }
    }
}

/// 服务发现文件：skill 读它构造请求；不存在 = ReSession 未运行
fn discovery_path() -> Option<std::path::PathBuf> {
    crate::settings::Settings::path().map(|p| p.with_file_name("bridge.json"))
}

/// 退出时清除（陈旧文件会让 skill 拿到"文件在但连不上"的困惑状态）
pub fn remove_discovery_file() {
    if let Some(path) = discovery_path() {
        match std::fs::remove_file(&path) {
            Ok(()) => log::info!("[bridge] discovery file removed"),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => log::warn!("[bridge] discovery remove failed: {e}"),
        }
    }
}

fn write_discovery(port: u16, token: &str) -> Result<(), String> {
    let path = discovery_path().ok_or("no home directory")?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let body = json!({ "port": port, "token": token }).to_string();
    std::fs::write(&path, body).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

/// 从请求头提取的 Authorization 值与期望 token 比对
fn token_ok(raw: Option<&str>, token: &str) -> bool {
    raw.map(|v| v == format!("Bearer {token}")).unwrap_or(false)
}

fn handle(app: &AppHandle, token: &str, mut req: Request) -> Result<(), std::io::Error> {
    let method = req.method().clone();
    let url = req.url().to_string();
    let (path, query) = match url.split_once('?') {
        Some((p, q)) => (p.to_string(), q.to_string()),
        None => (url, String::new()),
    };
    let auth = req
        .headers()
        .iter()
        .find(|h| h.field.as_str().as_str().eq_ignore_ascii_case("authorization"))
        .map(|h| h.value.as_str().to_string());

    let (status, body): (u16, Value) = if !token_ok(auth.as_deref(), token) {
        (401, json!({ "error": "unauthorized" }))
    } else {
        match (method, path.as_str()) {
            (Method::Get, "/bridge/sessions") => match list_sessions(app) {
                Ok(v) => (200, json!({ "sessions": v })),
                Err(e) => (500, json!({ "error": e })),
            },
            (Method::Post, "/bridge/send") => {
                let mut raw = String::new();
                let _ = std::io::Read::read_to_string(req.as_reader(), &mut raw);
                match serde_json::from_str::<SendBody>(&raw) {
                    Ok(b) => match send(app, b) {
                        Ok(v) => (200, v),
                        Err(e) => e,
                    },
                    Err(e) => (400, json!({ "error": format!("bad body: {e}") })),
                }
            }
            (Method::Get, "/bridge/tail") => match tail(&query) {
                Ok(v) => (200, v),
                Err(e) => e,
            },
            _ => (404, json!({ "error": "not found" })),
        }
    };

    let payload = serde_json::to_string(&body).unwrap_or_else(|_| "{}".into());
    let content_type = Header::from_bytes("Content-Type", "application/json")
        .expect("static header is valid");
    req.respond(
        Response::from_string(payload)
            .with_status_code(status)
            .with_header(content_type),
    )
}

/// 扫描缓存 + PTY 忙闲 → 可投递目标列表
fn list_sessions(app: &AppHandle) -> Result<Vec<BridgeSession>, String> {
    let settings = app.state::<SettingsState>();
    let busy_ms = settings.0.lock().unwrap().busy_ms;
    let metas: Vec<SessionMeta> = crate::scan_all_sessions(&settings)?;
    drop(settings);
    let statuses = pty::list(&app.state::<PtyMap>());
    let now = now_ms();
    Ok(metas
        .into_iter()
        .map(|m| {
            let id = format!("{}:{}", m.provider, m.id);
            let status = statuses.iter().find(|s| s.id == id);
            let running = status.is_some();
            let busy = running
                && now.saturating_sub(status.unwrap().last_output_ms) < busy_ms;
            BridgeSession {
                id,
                provider: m.provider.clone(),
                title: m.title.clone().unwrap_or_default(),
                cwd: m.cwd.clone().unwrap_or_default(),
                running,
                busy,
            }
        })
        .collect())
}

/// 投递：忙 → 409（不排队不吞）；空闲 → bracketed paste 一次写入
fn send(app: &AppHandle, body: SendBody) -> Result<Value, (u16, Value)> {
    let busy_ms = app.state::<SettingsState>().0.lock().unwrap().busy_ms;
    let ptys = app.state::<PtyMap>();
    let (last_output, cwd) = pty::output_state(&ptys, &body.target_id).ok_or((
        404,
        json!({ "error": "target not running", "targetId": body.target_id }),
    ))?;
    if now_ms().saturating_sub(last_output) < busy_ms {
        return Err((
            409,
            json!({ "error": "target busy", "targetId": body.target_id, "busyMs": busy_ms }),
        ));
    }
    pty::write(&ptys, &body.target_id, &wrap_bracketed_paste(&body.content))
        .map_err(|e| (500, json!({ "error": e })))?;
    log::info!(
        "[bridge] delivered {} chars -> {}",
        body.content.len(),
        body.target_id
    );
    Ok(json!({ "delivered": true, "targetId": body.target_id, "cwd": cwd }))
}

/// bracketed paste 包裹：保多行、单次提交（与 tmux-agent core.sh 同款）
fn wrap_bracketed_paste(content: &str) -> String {
    format!("\x1b[200~{}\x1b[201~\r", content)
}

/// 回读：目标会话原生 jsonl 的结构化增量（不刮终端屏幕）
fn tail(query: &str) -> Result<Value, (u16, Value)> {
    let target_id =
        query_param(query, "targetId").ok_or((400, json!({ "error": "missing targetId" })))?;
    let after_index: i64 = query_param(query, "afterIndex")
        .and_then(|v| v.parse().ok())
        .unwrap_or(-1);
    let limit: usize = query_param(query, "limit")
        .and_then(|v| v.parse().ok())
        .unwrap_or(50);
    let (provider, sess_id) = target_id
        .split_once(':')
        .ok_or((400, json!({ "error": "targetId must be <provider>:<id>" })))?;

    let provider_box = crate::provider_for(provider).map_err(|e| (400, json!({ "error": e })))?;
    let meta: SessionMeta = provider_box
        .scan()
        .map_err(|e| (500, json!({ "error": e.to_string() })))?
        .into_iter()
        .find(|m| m.id == sess_id)
        .ok_or((404, json!({ "error": "session not found", "targetId": target_id })))?;
    let events = provider_box
        .load_transcript(&meta)
        .map_err(|e| (500, json!({ "error": e.to_string() })))?;

    let all: Vec<TailMessage> = events
        .iter()
        .enumerate()
        .filter(|(_, ev)| !ev.sidechain && matches!(ev.role, Role::User | Role::Assistant))
        .map(|(i, ev)| TailMessage {
            index: i,
            role: match ev.role {
                Role::User => "user",
                Role::Assistant => "assistant",
                Role::System => "system",
            },
            text: text_of(&ev.blocks),
            timestamp: ev.timestamp.clone(),
        })
        // 纯工具调用事件没有正文，对轮询方（等最终回复）是噪音；index 仍按
        // 事件原始位置，与 latestIndex 坐标系一致
        .filter(|m| !m.text.trim().is_empty())
        .collect();
    let latest_index = events.len().saturating_sub(1) as i64;
    let messages = filter_tail(all, after_index, limit);
    Ok(json!({ "messages": messages, "latestIndex": latest_index }))
}

fn text_of(blocks: &[Block]) -> String {
    blocks
        .iter()
        .filter_map(|b| match b {
            Block::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// after_index 之后的消息，最多保留最近 `limit` 条
fn filter_tail(all: Vec<TailMessage>, after_index: i64, limit: usize) -> Vec<TailMessage> {
    let mut picked: Vec<TailMessage> = all
        .into_iter()
        .filter(|m| m.index as i64 > after_index)
        .collect();
    if picked.len() > limit {
        picked.drain(..picked.len() - limit);
    }
    picked
}

/// 极简 query 解析（id 均为 provider:uuid / 数字，无需百分号解码）
fn query_param<'a>(query: &'a str, key: &str) -> Option<&'a str> {
    query.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        (k == key).then_some(v)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paste_wraps_multiline_and_submits_once() {
        let wrapped = wrap_bracketed_paste("line1\n\nline3");
        assert!(wrapped.starts_with("\x1b[200~"));
        assert!(wrapped.ends_with("\x1b[201~\r"));
        assert_eq!(wrapped.matches('\r').count(), 1, "只允许一次提交回车");
        assert!(wrapped.contains("line1\n\nline3"));
    }

    #[test]
    fn token_requires_exact_bearer_match() {
        assert!(token_ok(Some("Bearer abc"), "abc"));
        assert!(!token_ok(Some("Bearer xyz"), "abc"));
        assert!(!token_ok(Some("bearer abc"), "abc"));
        assert!(!token_ok(Some("abc"), "abc"));
        assert!(!token_ok(None, "abc"));
    }

    #[test]
    fn query_param_reads_keys() {
        let q = "targetId=codex%3Aabc&afterIndex=3&limit=10";
        assert_eq!(query_param(q, "targetId"), Some("codex%3Aabc"));
        assert_eq!(query_param(q, "afterIndex"), Some("3"));
        assert_eq!(query_param(q, "missing"), None);
    }

    fn msg(index: usize, text: &str) -> TailMessage {
        TailMessage {
            index,
            role: "assistant",
            text: text.into(),
            timestamp: None,
        }
    }

    #[test]
    fn tail_filters_after_index_and_keeps_latest() {
        let all: Vec<TailMessage> = (0..10).map(|i| msg(i, "m")).collect();
        let picked = filter_tail(all, 4, 100);
        assert_eq!(picked.first().unwrap().index, 5);

        let all: Vec<TailMessage> = (0..10).map(|i| msg(i, "m")).collect();
        let picked = filter_tail(all, -1, 3);
        assert_eq!(
            picked.iter().map(|m| m.index).collect::<Vec<_>>(),
            vec![7, 8, 9],
            "limit 截断保留最近的消息"
        );
    }

    /// 契约：BridgeSession 必须 camelCase（同 SessionMeta/PtyStatus 教训）
    #[test]
    fn bridge_session_keys_are_camel_case() {
        let s = BridgeSession {
            id: "claude:x".into(),
            provider: "claude".into(),
            title: "t".into(),
            cwd: "c".into(),
            running: true,
            busy: false,
        };
        let v = serde_json::to_value(&s).unwrap();
        let mut keys: Vec<&str> = v.as_object().unwrap().keys().map(|k| k.as_str()).collect();
        keys.sort_unstable();
        // serde_json::Value 内部是 BTreeMap：key 按字母序重排，顺序不是契约的一部分
        assert_eq!(keys, ["busy", "cwd", "id", "provider", "running", "title"]);
        assert!(!serde_json::to_string(&s).unwrap().contains('_'));
    }
}
