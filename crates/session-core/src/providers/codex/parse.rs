//! Read Codex rollout JSONL without modifying native session data.
//! The format is private and evolves; unknown rows and malformed lines are skipped.

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

#[derive(Deserialize)]
struct RawLine {
    #[serde(rename = "type")]
    kind: Option<String>,
    timestamp: Option<String>,
    #[serde(default)]
    payload: Value,
}

// Search does not need tool arguments/outputs, which dominate the size of some
// rollout files. Serde skips those fields without allocating their contents.
#[derive(Default, Deserialize)]
struct SearchPayload {
    #[serde(rename = "type")]
    kind: Option<String>,
    role: Option<String>,
    content: Option<Value>,
}

#[derive(Deserialize)]
struct SearchRawLine {
    #[serde(rename = "type")]
    kind: Option<String>,
    #[serde(default)]
    payload: SearchPayload,
}

fn lines(path: &Path) -> std::io::Result<impl Iterator<Item = RawLine>> {
    let reader = BufReader::new(File::open(path)?);
    // Split into bytes first: one invalid UTF-8 row must not hide later valid rows.
    Ok(reader
        .split(b'\n')
        .filter_map(Result::ok)
        .filter_map(|line| serde_json::from_slice(&line).ok()))
}

const MAX_BRIEF: usize = 200;
const SNIPPET_CHARS: usize = 60;
const MAX_META_BYTES: usize = 1_000_000;

fn truncate(value: &str) -> String {
    let clean: String = value.chars().filter(|c| !c.is_control()).collect();
    if clean.chars().count() <= MAX_BRIEF {
        clean
    } else {
        format!("{}…", clean.chars().take(MAX_BRIEF).collect::<String>())
    }
}

fn brief(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(text)) => truncate(text),
        Some(other) => truncate(&other.to_string()),
        None => String::new(),
    }
}

fn role(value: &str) -> Option<Role> {
    match value {
        "user" => Some(Role::User),
        "assistant" => Some(Role::Assistant),
        _ => None,
    }
}

fn message_blocks(payload: &Value) -> Vec<Block> {
    let Some(content) = payload.get("content") else {
        return Vec::new();
    };
    content_blocks(content)
}

fn content_blocks(content: &Value) -> Vec<Block> {
    if let Some(text) = content.as_str() {
        return vec![Block::Text { text: text.into() }];
    }
    content
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|part| {
            match part.get("type").and_then(Value::as_str) {
                Some("input_text" | "output_text" | "text") => part
                    .get("text")
                    .and_then(Value::as_str)
                    .map(|text| Block::Text { text: text.into() }),
                _ => None, // image/audio and future content types cannot be rendered by IR v0
            }
        })
        .collect()
}

fn event_from_line(line: RawLine) -> Option<Event> {
    if line.kind.as_deref() != Some("response_item") {
        return None;
    }
    let payload = &line.payload;
    let kind = payload.get("type")?.as_str()?;
    let (role, blocks) = match kind {
        "message" => {
            let role = role(payload.get("role")?.as_str()?)?;
            (role, message_blocks(payload))
        }
        "function_call" | "custom_tool_call" => (
            Role::Assistant,
            vec![Block::ToolUse {
                name: payload
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("tool")
                    .into(),
                brief: brief(payload.get("arguments").or_else(|| payload.get("input"))),
            }],
        ),
        "function_call_output" | "custom_tool_call_output" => (
            Role::Assistant,
            vec![Block::ToolResult {
                brief: brief(payload.get("output")),
            }],
        ),
        _ => return None,
    };
    if blocks.is_empty() {
        return None;
    }
    Some(Event {
        role,
        timestamp: line.timestamp,
        blocks,
        sidechain: false,
    })
}

fn project_dir(cwd: Option<&str>) -> String {
    let Some(path) = cwd else { return "Codex".into() };
    let normalized = path.replace('\\', "/");
    normalized
        .split('/')
        .filter(|part| !part.is_empty())
        .last()
        .unwrap_or("Codex")
        .into()
}

struct CachedMeta {
    mtime: SystemTime,
    len: u64,
    meta: SessionMeta,
}

fn meta_cache() -> &'static Mutex<HashMap<PathBuf, CachedMeta>> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, CachedMeta>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn scan_file(path: &Path) -> std::io::Result<SessionMeta> {
    let metadata = std::fs::metadata(path)?;
    let mtime = metadata.modified()?;
    let len = metadata.len();
    let cached = {
        let cache = meta_cache().lock().unwrap();
        cache
            .get(path)
            .filter(|hit| hit.mtime == mtime && hit.len == len)
            .map(|hit| hit.meta.clone())
    };
    if let Some(meta) = cached {
        return Ok(meta);
    }

    let mut id: Option<String> = None;
    let mut cwd: Option<String> = None;
    let mut branch: Option<String> = None;
    let mut created_at: Option<String> = None;
    let mut first_user: Option<String> = None;
    let mut message_count = 0usize;
    let mut reader = BufReader::new(File::open(path)?);
    let mut source_line = Vec::new();
    let mut scanned_bytes = 0usize;
    let large = len > MAX_META_BYTES as u64;
    loop {
        source_line.clear();
        let bytes = reader.read_until(b'\n', &mut source_line)?;
        if bytes == 0 {
            break;
        }
        scanned_bytes += bytes;
        let Ok(line) = serde_json::from_slice::<RawLine>(&source_line) else {
            continue;
        };
        if line.kind.as_deref() == Some("session_meta") {
            let payload = &line.payload;
            id = payload.get("id").and_then(Value::as_str).map(str::to_string);
            cwd = payload.get("cwd").and_then(Value::as_str).map(str::to_string);
            branch = payload
                .pointer("/git/branch")
                .and_then(Value::as_str)
                .map(str::to_string);
            created_at = payload
                .get("timestamp")
                .and_then(Value::as_str)
                .map(str::to_string)
                .or(line.timestamp);
            continue;
        }
        if line.kind.as_deref() == Some("turn_context") {
            if let Some(current_cwd) = line.payload.get("cwd").and_then(Value::as_str) {
                cwd = Some(current_cwd.to_string());
            }
            continue;
        }
        if line.kind.as_deref() == Some("response_item")
            && line.payload.get("type").and_then(Value::as_str) == Some("message")
        {
            if let Some(message_role) = line.payload.get("role").and_then(Value::as_str) {
                if role(message_role).is_some() {
                    message_count += 1;
                    if message_role == "user" && first_user.is_none() {
                        first_user = line
                            .payload
                            .get("content")
                            .and_then(Value::as_array)
                            .into_iter()
                            .flatten()
                            .filter_map(|part| part.get("text").and_then(Value::as_str))
                            .map(|text| truncate(text.trim()))
                            .find(|title| !title.is_empty());
                    }
                }
            }
        }
        // Large Codex rollouts can be hundreds of MB. The list needs metadata,
        // not an exact message count; search/replay read the full file on demand.
        if large && id.is_some() && (first_user.is_some() || scanned_bytes >= MAX_META_BYTES) {
            break;
        }
    }
    let id = id.ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, "missing session_meta id")
    })?;
    let meta = SessionMeta {
        provider: "codex".into(),
        id,
        project_dir: project_dir(cwd.as_deref()),
        cwd,
        title: first_user,
        created_at: created_at.clone(),
        modified_at: mtime
            .duration_since(std::time::UNIX_EPOCH)
            .ok()
            .map(|duration| format_secs(duration.as_secs()))
            .or(created_at),
        git_branch: branch,
        message_count: if large { 0 } else { message_count },
        source_file: path.to_path_buf(),
    };
    meta_cache().lock().unwrap().insert(
        path.to_path_buf(),
        CachedMeta {
            mtime,
            len,
            meta: meta.clone(),
        },
    );
    Ok(meta)
}

fn format_secs(secs: u64) -> String {
    let days = secs / 86_400;
    let rem = secs % 86_400;
    let (year, month, day) = civil_from_days(days as i64);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if month <= 2 { year + 1 } else { year }, month, day)
}

pub fn load_events(path: &Path) -> std::io::Result<Vec<Event>> {
    Ok(lines(path)?.filter_map(event_from_line).collect())
}

#[derive(Clone)]
struct SearchLine {
    index: usize,
    role: Role,
    text: String,
}

struct CachedText {
    mtime: SystemTime,
    len: u64,
    lines: Vec<SearchLine>,
    event_count: usize,
}

fn text_cache() -> &'static Mutex<HashMap<PathBuf, CachedText>> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, CachedText>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn searchable_lines(path: &Path) -> std::io::Result<(Vec<SearchLine>, usize)> {
    let mut lines_with_text = Vec::new();
    let mut event_count = 0usize;
    for line in BufReader::new(File::open(path)?).split(b'\n').filter_map(Result::ok) {
        let Ok(raw) = serde_json::from_slice::<SearchRawLine>(&line) else {
            continue;
        };
        if raw.kind.as_deref() != Some("response_item") {
            continue;
        }
        match raw.payload.kind.as_deref() {
            Some("message") => {
                let Some(message_role) = raw.payload.role.as_deref().and_then(role) else {
                    continue;
                };
                let Some(content) = raw.payload.content.as_ref() else {
                    continue;
                };
                let blocks = content_blocks(content);
                if blocks.is_empty() {
                    continue;
                }
                let index = event_count;
                event_count += 1;
                let text = blocks
                    .into_iter()
                    .filter_map(|block| match block {
                        Block::Text { text } => Some(text),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                if !text.is_empty() {
                    lines_with_text.push(SearchLine {
                        index,
                        role: message_role,
                        text,
                    });
                }
            }
            Some("function_call" | "custom_tool_call" | "function_call_output" | "custom_tool_call_output") => {
                event_count += 1;
            }
            _ => {}
        }
    }
    Ok((lines_with_text, event_count))
}

/// Return text hits and the length of this rollout's complete IR event stream.
/// The caller uses the length to offset hit indices across paginated rollout files.
pub fn search_file(meta: &SessionMeta, query: &str) -> (Vec<SearchHit>, usize) {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return (Vec::new(), 0);
    }
    let Ok(metadata) = std::fs::metadata(&meta.source_file) else {
        return (Vec::new(), 0);
    };
    let Ok(mtime) = metadata.modified() else {
        return (Vec::new(), 0);
    };
    let len = metadata.len();
    let mut cache = text_cache().lock().unwrap();
    let needs_refresh = !matches!(
        cache.get(&meta.source_file),
        Some(hit) if hit.mtime == mtime && hit.len == len
    );
    if needs_refresh {
        let Ok((lines, event_count)) = searchable_lines(&meta.source_file) else {
            return (Vec::new(), 0);
        };
        cache.insert(
            meta.source_file.clone(),
            CachedText { mtime, len, lines, event_count },
        );
    }
    let entry = cache.get(&meta.source_file).expect("search cache populated");
    let hits = entry
        .lines
        .iter()
        .filter_map(|line| {
            let lower = line.text.to_lowercase();
            let pos = lower.find(&q)?;
            let start = lower[..pos].chars().count();
            let chars: Vec<char> = line.text.chars().collect();
            let from = start.saturating_sub(SNIPPET_CHARS);
            let to = (start + q.chars().count() + SNIPPET_CHARS).min(chars.len());
            let mut snippet = String::new();
            if from > 0 {
                snippet.push('…');
            }
            snippet.extend(&chars[from..to]);
            if to < chars.len() {
                snippet.push('…');
            }
            Some(SearchHit {
                session: meta.clone(),
                event_index: line.index,
                role: line.role,
                sidechain: false,
                snippet,
            })
        })
        .collect();
    (hits, entry.event_count)
}
