//! JSONL 宽容解析：单行失败跳过、未知类型忽略、字段缺失降级。
//! 格式实测笔记见 docs/data-formats.md。

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;

use serde::Deserialize;
use serde_json::Value;

use crate::ir::{Block, Event, Role, SessionMeta};
use crate::provider::SearchHit;

/// 需要的行字段都是 Option：格式演化时新字段/缺字段都不炸。
#[derive(Deserialize)]
struct RawLine {
    #[serde(rename = "type")]
    kind: Option<String>,
    #[serde(default)]
    message: Option<RawMessage>,
    timestamp: Option<String>,
    cwd: Option<String>,
    summary: Option<String>,
    #[serde(rename = "customTitle")]
    custom_title: Option<String>,
    #[serde(rename = "isSidechain")]
    is_sidechain: Option<bool>,
}

#[derive(Deserialize)]
struct RawMessage {
    /// 观测：可能是 string，也可能是 block 数组 —— 用 Value 兜住。
    /// （role 不从 message 取，直接由行 type 决定）
    content: Option<Value>,
}

const MAX_BRIEF: usize = 200;

fn truncate(s: &str) -> String {
    let s: String = s.chars().filter(|c| !c.is_control()).collect();
    if s.chars().count() <= MAX_BRIEF {
        s
    } else {
        let t: String = s.chars().take(MAX_BRIEF).collect();
        format!("{t}…")
    }
}

fn role_of(kind: &str) -> Option<Role> {
    match kind {
        "user" => Some(Role::User),
        "assistant" => Some(Role::Assistant),
        _ => None,
    }
}

/// content string 或 block 数组 → IR blocks；未知项跳过。
fn content_to_blocks(content: Option<&Value>) -> Vec<Block> {
    let Some(v) = content else { return Vec::new() };
    match v {
        Value::String(s) => vec![Block::Text { text: s.clone() }],
        Value::Array(items) => items
            .iter()
            .filter_map(|item| {
                let kind = item.get("type")?.as_str()?;
                match kind {
                    "text" => Some(Block::Text {
                        text: item.get("text")?.as_str()?.to_string(),
                    }),
                    "tool_use" => Some(Block::ToolUse {
                        name: item.get("name")?.as_str()?.to_string(),
                        brief: item
                            .get("input")
                            .map(|i| truncate(&i.to_string()))
                            .unwrap_or_default(),
                    }),
                    "tool_result" => Some(Block::ToolResult {
                        brief: item
                            .get("content")
                            .map(|c| match c {
                                Value::String(s) => truncate(s),
                                other => truncate(&other.to_string()),
                            })
                            .unwrap_or_default(),
                    }),
                    _ => None,
                }
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// 逐行读取原始行（共享给扫描与转录，跳过无法解析的行）。
fn read_lines(path: &Path) -> std::io::Result<impl Iterator<Item = RawLine>> {
    let reader = BufReader::new(File::open(path)?);
    Ok(reader.lines().map_while(Result::ok).filter_map(|line| {
        // 宽容：坏行直接丢弃
        serde_json::from_str::<RawLine>(&line).ok()
    }))
}

/// mtime+size 元数据缓存：扫描只重读变过的文件（性能设计见 architecture.md 3.1）
struct CachedMeta {
    mtime: SystemTime,
    len: u64,
    meta: SessionMeta,
}

fn cache() -> &'static Mutex<HashMap<PathBuf, CachedMeta>> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, CachedMeta>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 流式扫描单个会话文件，提取列表所需的元数据（不保留正文）。
pub fn scan_session_file(path: &Path, project_dir: &str) -> std::io::Result<SessionMeta> {
    let md = std::fs::metadata(path)?;
    let mtime = md.modified()?;
    let len = md.len();
    {
        let c = cache().lock().unwrap();
        if let Some(hit) = c.get(path) {
            if hit.mtime == mtime && hit.len == len {
                return Ok(hit.meta.clone());
            }
        }
    }
    let id = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_string();

    let mut title: Option<String> = None; // /rename → custom-title 行（实测格式见 data-formats.md）
    let mut summary: Option<String> = None;
    let mut first_user_text: Option<String> = None;
    let mut cwd: Option<String> = None;
    let mut created_at: Option<String> = None;
    let mut message_count = 0usize;

    for raw in read_lines(path)? {
        let kind = raw.kind.as_deref().unwrap_or("");
        if cwd.is_none() {
            cwd = raw.cwd.clone();
        }
        if created_at.is_none() {
            created_at = raw.timestamp.clone();
        }
        match kind {
            "summary" => {
                // 重复出现取最后一次（compact 会刷新摘要）
                if raw.summary.is_some() {
                    summary = raw.summary.clone();
                }
            }
            // /rename 写入的会话名，后写覆盖前写
            "custom-title" => {
                if raw.custom_title.is_some() {
                    title = raw.custom_title.clone();
                }
            }
            "user" | "assistant" => {
                message_count += 1;
                if kind == "user" && first_user_text.is_none() && raw.is_sidechain != Some(true) {
                    // 首条用户消息作为标题兜底；排队操作等非正文行没有 message
                    if let Some(Block::Text { text }) =
                        content_to_blocks(raw.message.as_ref().and_then(|m| m.content.as_ref()))
                            .into_iter()
                            .next()
                    {
                        if !text.trim().is_empty() {
                            first_user_text = Some(truncate(&text));
                        }
                    }
                }
            }
            _ => {} // summary/attachment/queue-operation/未知类型：按需忽略
        }
    }

    // modified 取文件系统时间戳（追加写 ⇒ mtime 即最后活动时间）
    let modified_at = mtime
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| format_secs(d.as_secs()));

    let meta = SessionMeta {
        provider: "claude".into(),
        id,
        cwd,
        project_dir: project_dir.to_string(),
        title: title.or(summary).or(first_user_text),
        created_at,
        modified_at,
        message_count,
        source_file: path.to_path_buf(),
    };
    cache().lock().unwrap().insert(
        path.to_path_buf(),
        CachedMeta {
            mtime,
            len,
            meta: meta.clone(),
        },
    );
    Ok(meta)
}

/// 完整转录 → IR。只保留 user/assistant 消息，其余行类型忽略。
pub fn load_events(path: &Path) -> std::io::Result<Vec<Event>> {
    let mut events = Vec::new();
    for raw in read_lines(path)? {
        let kind = raw.kind.as_deref().unwrap_or("");
        let Some(role) = role_of(kind) else { continue };
        let blocks = content_to_blocks(raw.message.as_ref().and_then(|m| m.content.as_ref()));
        if blocks.is_empty() {
            continue;
        }
        events.push(Event {
            role,
            timestamp: raw.timestamp,
            blocks,
            sidechain: raw.is_sidechain.unwrap_or(false),
        });
    }
    Ok(events)
}

/// 可搜索文本的一行（一个含文本块的事件）
#[derive(Clone)]
struct CachedLine {
    event_index: usize,
    role: Role,
    sidechain: bool,
    text: String,
}

struct CachedText {
    mtime: SystemTime,
    len: u64,
    lines: Vec<CachedLine>,
}

fn text_cache() -> &'static Mutex<HashMap<PathBuf, CachedText>> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, CachedText>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 提取可搜索文本：仅 user/assistant 的 Text 块（设计确认：工具块不搜）。
/// event_index 是在完整事件流（含工具事件）中的下标，v2 定位用。
fn load_searchable(path: &Path) -> std::io::Result<Vec<CachedLine>> {
    Ok(load_events(path)?
        .into_iter()
        .enumerate()
        .filter_map(|(i, e)| {
            let text: Vec<String> = e
                .blocks
                .into_iter()
                .filter_map(|b| match b {
                    Block::Text { text } => Some(text),
                    _ => None,
                })
                .collect();
            if text.is_empty() {
                None
            } else {
                Some(CachedLine {
                    event_index: i,
                    role: e.role,
                    sidechain: e.sidechain,
                    text: text.join("\n"),
                })
            }
        })
        .collect())
}

const SNIPPET_CHARS: usize = 60;

/// 在单个会话的可搜索文本中查找 query（大小写不敏感），每条消息最多一命中。
/// 匹配基于 to_lowercase：主流内容字符一一对应；极少数特殊 Unicode（如 İ）
/// 的字符数可能漂移，片段截取按字符边界做了保护，最坏情况偏移一位。
pub fn search_file(meta: &SessionMeta, query: &str) -> Vec<SearchHit> {
    let Ok(md) = std::fs::metadata(&meta.source_file) else {
        return vec![];
    };
    let Ok(mtime) = md.modified() else {
        return vec![];
    };
    let len = md.len();
    let lines = {
        let mut cache = text_cache().lock().unwrap();
        match cache.get(&meta.source_file) {
            Some(c) if c.mtime == mtime && c.len == len => c.lines.clone(),
            _ => {
                let lines = load_searchable(&meta.source_file).unwrap_or_default();
                cache.insert(
                    meta.source_file.clone(),
                    CachedText {
                        mtime,
                        len,
                        lines: lines.clone(),
                    },
                );
                lines
            }
        }
    };

    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return vec![];
    }
    let mut hits = Vec::new();
    for line in lines {
        let lower = line.text.to_lowercase();
        let Some(pos) = lower.find(&q) else {
            continue;
        };
        let start = lower[..pos].chars().count();
        let chars: Vec<char> = line.text.chars().collect();
        let s = start.saturating_sub(SNIPPET_CHARS);
        let e = (start + q.chars().count() + SNIPPET_CHARS).min(chars.len());
        let mut snippet = String::new();
        if s > 0 {
            snippet.push('…');
        }
        snippet.extend(&chars[s..e]);
        if e < chars.len() {
            snippet.push('…');
        }
        hits.push(SearchHit {
            session: meta.clone(),
            event_index: line.event_index,
            role: line.role,
            sidechain: line.sidechain,
            snippet,
        });
    }
    hits
}

fn format_secs(secs: u64) -> String {
    // 简化 ISO 秒（精度足够排序与展示），避免引入 chrono
    let days = secs / 86_400;
    let rem = secs % 86_400;
    // 自 1970-01-01 起的天数 → y/m/d（民用历算法）
    let (y, m, d) = civil_from_days(days as i64);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_string_becomes_single_text_block() {
        let v = Value::String("hello".into());
        let blocks = content_to_blocks(Some(&v));
        assert_eq!(blocks, vec![Block::Text { text: "hello".into() }]);
    }

    #[test]
    fn assistant_blocks_map_to_ir() {
        let v = serde_json::json!([
            {"type": "text", "text": "thinking..."},
            {"type": "tool_use", "name": "Bash", "input": {"command": "ls"}},
            {"type": "tool_result", "content": "file1\nfile2"},
            {"type": "image", "source": {}}, // 未知类型应被跳过
            {"type": "text", "text": "done"}
        ]);
        let blocks = content_to_blocks(Some(&v));
        assert_eq!(blocks.len(), 4);
        assert_eq!(
            blocks[1],
            Block::ToolUse {
                name: "Bash".into(),
                brief: "{\"command\":\"ls\"}".into(),
            }
        );
    }

    #[test]
    fn tool_result_brief_is_truncated() {
        let long = "x".repeat(500);
        let v = serde_json::json!([{ "type": "tool_result", "content": long }]);
        let blocks = content_to_blocks(Some(&v));
        match &blocks[0] {
            Block::ToolResult { brief } => assert!(brief.chars().count() == MAX_BRIEF + 1), // + ellipsis
            other => panic!("unexpected block: {other:?}"),
        }
    }

    #[test]
    fn scan_extracts_title_summary_and_count() {
        let dir = std::env::temp_dir().join("resession-test-scan");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("deadbeef-0000-0000-0000-000000000000.jsonl");
        std::fs::write(
            &file,
            concat!(
                "{\"type\":\"queue-operation\",\"operation\":\"enqueue\",\"timestamp\":\"2026-09-08T16:45:16.433Z\"}\n",
                "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"帮我修一下登录\"},",
                "\"timestamp\":\"2026-09-08T16:45:16.522Z\",\"cwd\":\"F:\\\\git-workspace\\\\ai\",\"isSidechain\":false}\n",
                "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"好的\"}]},",
                "\"timestamp\":\"2026-09-08T16:46:00.000Z\"}\n",
                "{\"type\":\"summary\",\"summary\":\"修复登录流程\"}\n",
                "NOT-VALID-JSON\n" // 坏行应被跳过
            ),
        )
        .unwrap();

        let meta = scan_session_file(&file, "F--git-workspace-ai").unwrap();
        assert_eq!(meta.id, "deadbeef-0000-0000-0000-000000000000");
        assert_eq!(meta.cwd.as_deref(), Some("F:\\git-workspace\\ai"));
        // title 兜底链：summary > 首条用户消息
        assert_eq!(meta.title.as_deref(), Some("修复登录流程"));
        assert_eq!(meta.message_count, 2);
        assert!(meta.modified_at.is_some());
        assert!(meta.created_at.as_deref().unwrap().starts_with("2026-09-08"));

        let events = load_events(&file).unwrap();
        assert_eq!(events.len(), 2); // queue-operation 与 summary 不进转录
        assert_eq!(events[0].role, Role::User);
        assert_eq!(
            events[0].blocks,
            vec![Block::Text { text: "帮我修一下登录".into() }]
        );

        std::fs::remove_file(&file).unwrap();
    }

    #[test]
    fn custom_title_wins_and_sidechain_flagged() {
        let dir = std::env::temp_dir().join("resession-test-title");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("babe0000-0000-0000-0000-000000000000.jsonl");
        std::fs::write(
            &file,
            concat!(
                "{\"type\":\"summary\",\"summary\":\"旧摘要\"}\n",
                "{\"type\":\"custom-title\",\"customTitle\":\"我的会话名\"}\n",
                "{\"type\":\"user\",\"isSidechain\":true,\"message\":{\"role\":\"user\",\"content\":\"内部消息\"},",
                "\"timestamp\":\"2026-09-08T10:00:00Z\"}\n",
                "{\"type\":\"user\",\"isSidechain\":false,\"message\":{\"role\":\"user\",\"content\":\"可见消息\"},",
                "\"timestamp\":\"2026-09-08T10:01:00Z\"}\n"
            ),
        )
        .unwrap();

        // 标题优先级：custom-title > summary
        let meta = scan_session_file(&file, "proj").unwrap();
        assert_eq!(meta.title.as_deref(), Some("我的会话名"));

        // 转录包含 sidechain 事件但带标记，由 UI 决定折叠展示
        let events = load_events(&file).unwrap();
        assert_eq!(events.len(), 2);
        assert!(events[0].sidechain);
        assert!(!events[1].sidechain);

        std::fs::remove_file(&file).unwrap();
    }

    #[test]
    fn search_finds_text_and_skips_tool_blocks() {
        let dir = std::env::temp_dir().join("resession-test-search");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("f00d0000-0000-0000-0000-000000000000.jsonl");
        std::fs::write(
            &file,
            concat!(
                "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"How do I fix the login flow?\"},",
                "\"timestamp\":\"2026-09-08T10:00:00Z\"}\n",
                "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"name\":\"Bash\",\"input\":{}}]}}\n",
                "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"The LOGIN issue is in auth.rs\"}]}}\n"
            ),
        )
        .unwrap();

        let meta = scan_session_file(&file, "p").unwrap();
        let hits = search_file(&meta, "login");
        assert_eq!(hits.len(), 2); // user 文本 + assistant 文本；工具块不进索引
        assert_eq!(hits[0].role, Role::User);
        assert!(hits[0].snippet.to_lowercase().contains("login"));
        assert_eq!(hits[1].event_index, 2); // 工具事件占了下标 1

        std::fs::remove_file(&file).unwrap();
    }

    #[test]
    fn sidechain_user_message_is_not_used_as_title() {
        let dir = std::env::temp_dir().join("resession-test-sidechain");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("cafe0000-0000-0000-0000-000000000000.jsonl");
        std::fs::write(
            &file,
            concat!(
                "{\"type\":\"user\",\"isSidechain\":true,\"message\":{\"role\":\"user\",\"content\":\"子agent内部消息\"},",
                "\"timestamp\":\"2026-09-08T10:00:00Z\"}\n",
                "{\"type\":\"user\",\"isSidechain\":false,\"message\":{\"role\":\"user\",\"content\":\"真正的首条消息\"},",
                "\"timestamp\":\"2026-09-08T10:01:00Z\"}\n"
            ),
        )
        .unwrap();

        let meta = scan_session_file(&file, "test-project").unwrap();
        assert_eq!(meta.title.as_deref(), Some("真正的首条消息"));

        std::fs::remove_file(&file).unwrap();
    }
}
