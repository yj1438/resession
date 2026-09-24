//! Codex local rollout adapter. Reads native JSONL and delegates execution to Codex CLI.

mod binary;
mod parse;

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::ir::{Event, SessionMeta};
use crate::provider::{ResumeSpec, ScanError, SearchHit, SessionProvider};

#[derive(Default)]
pub struct CodexProvider {
    root: Option<PathBuf>,
}

impl CodexProvider {
    pub fn with_root(root: PathBuf) -> Self {
        Self { root: Some(root) }
    }

    fn codex_home(&self) -> Result<PathBuf, ScanError> {
        if let Some(root) = &self.root {
            return Ok(root.parent().unwrap_or(root.as_path()).to_path_buf());
        }
        if let Some(path) = std::env::var_os("CODEX_HOME") {
            return Ok(PathBuf::from(path));
        }
        let home = std::env::var_os("USERPROFILE")
            .or_else(|| std::env::var_os("HOME"))
            .ok_or_else(|| ScanError::RootMissing("neither CODEX_HOME nor home is set".into()))?;
        Ok(PathBuf::from(home).join(".codex"))
    }

    fn sessions_root(&self) -> Result<PathBuf, ScanError> {
        let root = match &self.root {
            Some(root) => root.clone(),
            None => self.codex_home()?.join("sessions"),
        };
        if !root.is_dir() {
            return Err(ScanError::RootMissing(root.display().to_string()));
        }
        Ok(root)
    }

    /// One Codex thread may span multiple rollout files after a history handoff.
    /// Keep one list entry and retain every segment for replay/search.
    fn collect_sessions(&self) -> Result<Vec<(SessionMeta, Vec<PathBuf>)>, ScanError> {
        let root = self.sessions_root()?;
        let titles = load_titles(&self.codex_home()?.join("session_index.jsonl"));
        let mut by_id: HashMap<String, Vec<SessionMeta>> = HashMap::new();
        for_each_rollout(&root, |path| match parse::scan_file(&path) {
            Ok(meta) => by_id.entry(meta.id.clone()).or_default().push(meta),
            Err(error) => eprintln!("[codex-provider] skip {}: {error}", path.display()),
        })?;

        let mut sessions = Vec::new();
        for (id, mut parts) in by_id {
            parts.sort_by(|a, b| {
                a.created_at
                    .cmp(&b.created_at)
                    .then(a.source_file.cmp(&b.source_file))
            });
            let mut meta = parts[0].clone();
            let latest = parts.last().expect("nonempty by_id entry");
            meta.source_file = latest.source_file.clone();
            meta.modified_at = parts
                .iter()
                .filter_map(|part| part.modified_at.as_ref())
                .max()
                .cloned();
            meta.message_count = parts.iter().map(|part| part.message_count).sum();
            if let Some(part) = parts.iter().rev().find(|part| part.cwd.is_some()) {
                meta.cwd = part.cwd.clone();
                meta.project_dir = part.project_dir.clone();
            }
            if let Some(branch) = parts.iter().rev().find_map(|part| part.git_branch.as_ref()) {
                meta.git_branch = Some(branch.clone());
            }
            meta.title = titles
                .get(&id)
                .cloned()
                .or_else(|| parts.iter().find_map(|part| part.title.clone()));
            let paths = parts.into_iter().map(|part| part.source_file).collect();
            sessions.push((meta, paths));
        }
        sessions.sort_by(|a, b| b.0.modified_at.cmp(&a.0.modified_at));
        Ok(sessions)
    }
}

#[derive(Deserialize)]
struct IndexEntry {
    id: String,
    thread_name: String,
}

fn load_titles(index_path: &Path) -> HashMap<String, String> {
    let Ok(file) = File::open(index_path) else {
        return HashMap::new();
    };
    let mut titles = HashMap::new();
    for line in BufReader::new(file).lines().map_while(Result::ok) {
        if let Ok(entry) = serde_json::from_str::<IndexEntry>(&line) {
            if !entry.thread_name.trim().is_empty() {
                titles.insert(entry.id, entry.thread_name);
            }
        }
    }
    titles
}

fn for_each_rollout(root: &Path, mut visit: impl FnMut(PathBuf)) -> std::io::Result<()> {
    let mut directories = vec![root.to_path_buf()];
    while let Some(directory) = directories.pop() {
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) if directory.as_path() == root => return Err(error),
            Err(error) => {
                eprintln!("[codex-provider] skip {}: {error}", directory.display());
                continue;
            }
        };
        for entry in entries {
            let Ok(entry) = entry else { continue };
            let Ok(kind) = entry.file_type() else { continue };
            let path = entry.path();
            if kind.is_dir() {
                directories.push(path);
            } else if kind.is_file()
                && path.extension().and_then(|part| part.to_str()) == Some("jsonl")
            {
                visit(path);
            }
        }
    }
    Ok(())
}

impl SessionProvider for CodexProvider {
    fn name(&self) -> &'static str {
        "codex"
    }

    fn scan(&self) -> Result<Vec<SessionMeta>, ScanError> {
        Ok(self
            .collect_sessions()?
            .into_iter()
            .map(|(meta, _)| meta)
            .collect())
    }

    fn load_transcript(&self, meta: &SessionMeta) -> Result<Vec<Event>, ScanError> {
        let (_, paths) = self
            .collect_sessions()?
            .into_iter()
            .find(|(candidate, _)| candidate.id == meta.id)
            .ok_or_else(|| ScanError::RootMissing(meta.source_file.display().to_string()))?;
        let mut events = Vec::new();
        for path in paths {
            events.extend(parse::load_events(&path)?);
        }
        Ok(events)
    }

    fn resume_command(&self, meta: &SessionMeta) -> Result<ResumeSpec, ScanError> {
        binary::build_resume_spec(meta)
    }

    fn new_session_command(&self, cwd: PathBuf) -> Result<ResumeSpec, ScanError> {
        binary::build_new_session_spec(cwd)
    }

    fn search(&self, query: &str) -> Result<Vec<SearchHit>, ScanError> {
        let mut hits = Vec::new();
        for (meta, paths) in self.collect_sessions()? {
            let mut offset = 0usize;
            for path in paths {
                let mut part = meta.clone();
                part.source_file = path;
                let (segment_hits, event_count) = parse::search_file(&part, query);
                for mut hit in segment_hits {
                    hit.event_index += offset;
                    hit.session = meta.clone();
                    hits.push(hit);
                }
                offset += event_count;
            }
        }
        Ok(hits)
    }
}

pub fn set_binary_override(path: Option<String>) {
    binary::set_override(path);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    const ID: &str = "01234567-89ab-cdef-0123-456789abcdef";

    fn jsonl(rows: &[Value]) -> String {
        format!(
            "{}\n",
            rows.iter().map(|row| row.to_string()).collect::<Vec<_>>().join("\n")
        )
    }

    #[test]
    fn scans_searches_and_loads_a_synthetic_rollout() {
        let base = std::env::temp_dir().join(format!("resession-codex-test-{}", std::process::id()));
        let root = base.join("sessions");
        let dated = root.join("2026").join("09").join("24");
        fs::create_dir_all(&dated).unwrap();
        let path = dated.join(format!("rollout-2026-09-24T00-00-00-{ID}.jsonl"));
        let content = jsonl(&[
            json!({"timestamp":"2026-09-24T00:00:00Z","type":"session_meta",
                "payload":{"id":ID,"timestamp":"2026-09-24T00:00:00Z",
                    "cwd":"/tmp/project","git":{"branch":"main"}}}),
            json!({"timestamp":"2026-09-24T00:00:01Z","type":"response_item",
                "payload":{"type":"message","role":"user",
                    "content":[{"type":"input_text","text":"find the needle"}]}}),
            json!({"timestamp":"2026-09-24T00:00:02Z","type":"response_item",
                "payload":{"type":"function_call","name":"shell","arguments":"ls"}}),
            json!({"timestamp":"2026-09-24T00:00:03Z","type":"response_item",
                "payload":{"type":"function_call_output","output":"ok"}}),
            json!({"timestamp":"2026-09-24T00:00:04Z","type":"response_item",
                "payload":{"type":"message","role":"assistant",
                    "content":[{"type":"output_text","text":"needle found"}]}}),
            json!({"timestamp":"2026-09-24T00:00:05Z","type":"turn_context",
                "payload":{"cwd":"/tmp/project-latest"}}),
        ]);
        let (header, rest) = content.split_once('\n').unwrap();
        let mut bytes = format!("{header}\n").into_bytes();
        bytes.extend_from_slice(b"invalid \xff line\n");
        bytes.extend_from_slice(rest.as_bytes());
        bytes.extend_from_slice(b"malformed line\n");
        fs::write(&path, bytes).unwrap();
        let continuation = dated.join(format!(
            "rollout-2026-09-24T01-00-00-{ID}_fedcba98-7654-3210-fedc-ba9876543210.jsonl"
        ));
        fs::write(
            &continuation,
            jsonl(&[
                json!({"timestamp":"2026-09-24T01:00:00Z","type":"session_meta",
                    "payload":{"id":ID,"timestamp":"2026-09-24T01:00:00Z",
                        "cwd":"/tmp/project-latest","history_mode":"paginated",
                        "history_base":{"thread_id":ID,"end_byte_offset":100}}}),
                json!({"timestamp":"2026-09-24T01:00:01Z","type":"response_item",
                    "payload":{"type":"message","role":"user",
                        "content":[{"type":"input_text","text":"second segment"}]}}),
                json!({"timestamp":"2026-09-24T01:00:02Z","type":"response_item",
                    "payload":{"type":"message","role":"assistant",
                        "content":[{"type":"output_text","text":"continued"}]}}),
            ]),
        )
        .unwrap();
        fs::write(
            base.join("session_index.jsonl"),
            format!("{{\"id\":\"{ID}\",\"thread_name\":\"Search fixture\",\"updated_at\":\"2026-09-24T00:00:04Z\"}}\n"),
        )
        .unwrap();

        let provider = CodexProvider::with_root(root);
        let sessions = provider.scan().unwrap();
        assert_eq!(sessions.len(), 1);
        let meta = &sessions[0];
        assert_eq!(meta.id, ID);
        assert_eq!(meta.title.as_deref(), Some("Search fixture"));
        assert_eq!(meta.cwd.as_deref(), Some("/tmp/project-latest"));
        assert_eq!(meta.git_branch.as_deref(), Some("main"));
        assert_eq!(meta.message_count, 4);
        assert_eq!(meta.source_file, continuation);

        let events = provider.load_transcript(meta).unwrap();
        assert_eq!(events.len(), 6);
        let hits = provider.search("NEEDLE").unwrap();
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].event_index, 0);
        assert_eq!(hits[1].event_index, 3);
        let later = provider.search("second").unwrap();
        assert_eq!(later.len(), 1);
        assert_eq!(later[0].event_index, 4);
        fs::remove_dir_all(base).unwrap();
    }
}
