//! DTO 序列化契约测试。
//!
//! 约定：所有跨端（Rust → TS）结构体必须 `serde(rename_all = "camelCase")`，
//! 与 `src/types.ts` 的手写镜像逐字段对齐。历史上 SessionMeta / PtyStatus
//! 两次因漏写 rename 造成前端读 undefined——本文件就是防复发闸门。
//!
//! 新增 DTO 或字段时的三步：
//!   ① 确认 serde(rename_all = "camelCase")
//!   ② 在此补 key 列表断言
//!   ③ 同步 src/types.ts

use session_core::{Block, Event, Role, SearchHit, SessionMeta, SessionProvider};

/// key 列表（排序后返回）。顺序不是契约的一部分——serde_json::Value 默认
/// 用 BTreeMap 会按字母序重排，契约只关心"有哪些 key、长什么样"。
fn keys(v: &serde_json::Value) -> Vec<String> {
    let mut k: Vec<String> = v
        .as_object()
        .expect("DTO 必须序列化为 object")
        .keys()
        .cloned()
        .collect();
    k.sort();
    k
}

/// 递归断言：任何层级的 key 都不允许蛇形命名（漏 rename 的典型症状）
fn assert_no_snake(v: &serde_json::Value) {
    match v {
        serde_json::Value::Object(m) => {
            for (k, val) in m {
                assert!(
                    !k.contains('_'),
                    "DTO key `{k}` 含蛇形命名——跨端 DTO 必须统一 camelCase"
                );
                assert_no_snake(val);
            }
        }
        serde_json::Value::Array(a) => a.iter().for_each(assert_no_snake),
        _ => {}
    }
}

fn sample_meta() -> SessionMeta {
    SessionMeta {
        provider: "claude".into(),
        id: "u".into(),
        cwd: Some("C:\\x".into()),
        project_dir: "C--x".into(),
        title: Some("t".into()),
        created_at: None,
        modified_at: Some("2026-09-19T00:00:00Z".into()),
        message_count: 1,
        source_file: "c:\\x\\u.jsonl".into(),
    }
}

#[test]
fn session_meta_keys_are_camel_case() {
    let v = serde_json::to_value(&sample_meta()).unwrap();
    // 期望值按结构体声明顺序书写，比较前排序（排序规则 = 字节序，大写在前）
    let mut expected = [
        "provider", "id", "cwd", "projectDir", "title", "createdAt", "modifiedAt", "messageCount", "sourceFile",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect::<Vec<_>>();
    expected.sort();
    assert_eq!(keys(&v), expected);
    assert_no_snake(&v);
}

#[test]
fn event_role_and_block_tags_follow_the_contract() {
    let e = Event {
        role: Role::Assistant,
        timestamp: Some("t".into()),
        blocks: vec![
            Block::ToolUse {
                name: "Bash".into(),
                brief: "ls".into(),
            },
            Block::Text {
                text: "hi".into(),
            },
            Block::ToolResult { brief: "ok".into() },
        ],
        sidechain: true,
    };
    let v = serde_json::to_value(&e).unwrap();
    assert_eq!(keys(&v), ["blocks", "role", "sidechain", "timestamp"]);
    assert_eq!(v["role"], "assistant"); // Role 序列化为小写
    assert_eq!(v["blocks"][0]["kind"], "toolUse"); // Block tag 驼峰
    assert_eq!(v["blocks"][1]["kind"], "text");
    assert_eq!(v["blocks"][2]["kind"], "toolResult");
    assert_eq!(keys(&v["blocks"][0]), ["brief", "kind", "name"]);
    assert_no_snake(&v);
}

#[test]
fn search_hit_keys_are_camel_case() {
    let h = SearchHit {
        session: sample_meta(),
        event_index: 3,
        role: Role::User,
        sidechain: false,
        snippet: "…login…".into(),
    };
    let v = serde_json::to_value(&h).unwrap();
    assert_eq!(keys(&v), ["eventIndex", "role", "session", "sidechain", "snippet"]);
    assert_eq!(v["session"]["messageCount"], 1); // 内嵌 SessionMeta 同样走 camelCase
    assert_no_snake(&v);
}

/// SessionProvider 本身不跨端，但 registry 必须至少含 claude（防手滑清空）
#[test]
fn registry_nonempty() {
    assert!(!session_core::registry().is_empty());
}
