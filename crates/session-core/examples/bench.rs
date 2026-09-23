//! 性能基准：合成 100/500/1000 会话数据集，测量扫描/搜索/转录加载耗时。
//!
//! 运行：`cargo run --release --example bench`
//! 数据是确定性的合成 JSONL（中英文混排、工具调用、sidechain、summary/
//! custom-title 噪声），写入系统临时目录，跑完自动清理。不触碰真实会话。

use session_core::providers::claude::ClaudeProvider;
use session_core::SessionProvider;
use std::fmt::Write as _;
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

fn main() {
    println!("ReSession 扫描/搜索性能基准（release 构建）\n");
    let mut rows: Vec<String> = Vec::new();
    for n in [100usize, 500, 1000] {
        rows.extend(bench_dataset(n));
    }
    println!("\n## 结果汇总\n");
    println!(
        "| 数据集 | 文件数 | 体积 | 冷扫描 | 热扫描 | 冷搜索 | 热搜索 | 最大转录加载 |"
    );
    println!("|---|---|---|---|---|---|---|---|");
    for r in rows {
        println!("| {r} |");
    }
}

/// 时间戳：固定基准 + i 秒，格式贴近真实 jsonl
fn ts(i: usize) -> String {
    let s = i % 60;
    let m = (i / 60) % 60;
    let h = (i / 3600) % 24;
    format!("2026-09-01T{h:02}:{m:02}:{s:02}.000Z")
}

/// 拟真事件行。marker=true 时嵌入搜索基准词。
fn event_line(sid: &str, i: usize, kind: u8, marker: bool) -> String {
    let t = ts(i);
    let zh = format!("处理登录流程 step {i}：重构鉴权模块的边界检查");
    match kind {
        0 => {
            let text = if marker {
                format!("{zh}，另外查一下缝纫机基准标记 {i}")
            } else {
                zh
            };
            format!(
                "{{\"type\":\"user\",\"message\":{{\"role\":\"user\",\"content\":\"{text}\"}},\
                 \"uuid\":\"u{i}\",\"parentUuid\":null,\"timestamp\":\"{t}\",\
                 \"cwd\":\"D:\\\\bench\\\\proj\",\"gitBranch\":\"feature/bench\",\
                 \"isSidechain\":false,\"sessionId\":\"{sid}\"}}\n"
            )
        }
        1 => format!(
            "{{\"type\":\"assistant\",\"message\":{{\"role\":\"assistant\",\"content\":[\
             {{\"type\":\"text\",\"text\":\"分析完成 step {i}，结论是重构 pay_service\"}},\
             {{\"type\":\"tool_use\",\"name\":\"Bash\",\"input\":{{\"command\":\"cargo test {i}\"}}}}]}},\
             \"timestamp\":\"{t}\",\"sessionId\":\"{sid}\"}}\n"
        ),
        _ => {
            let sidechain = if i % 10 == 0 { "true" } else { "false" };
            format!(
                "{{\"type\":\"assistant\",\"message\":{{\"role\":\"assistant\",\"content\":[\
                 {{\"type\":\"tool_result\",\"content\":\"ok ({i} tests passed)\"}}]}},\
                 \"isSidechain\":{sidechain},\"timestamp\":\"{t}\",\"sessionId\":\"{sid}\"}}\n"
            )
        }
    }
}

/// 写一个会话文件，返回 (事件数, 字节数)。events=0 时写一行垃圾行（降级路径样本）。
fn write_session(dir: &Path, sid: &str, events: usize, marker: bool) -> (usize, u64) {
    let mut body = String::with_capacity(events * 300);
    if events == 0 {
        body.push_str("total garbage line\n");
        let p = dir.join(format!("{sid}.jsonl"));
        let mut f = fs::File::create(&p).unwrap();
        f.write_all(body.as_bytes()).unwrap();
        return (0, 0);
    }
    body.push_str(&format!(
        "{{\"type\":\"queue-operation\",\"operation\":\"enqueue\",\"timestamp\":\"{}\"}}\n",
        ts(0)
    ));
    if marker {
        body.push_str(
            "{\"type\":\"custom-title\",\"customTitle\":\"缝纫机基准会话\",\"sessionId\":\"x\"}\n",
        );
    }
    for k in 0..events {
        body.push_str(&event_line(sid, k, (k % 3) as u8, marker && k == events / 2));
    }
    if marker {
        body.push_str("{\"type\":\"summary\",\"summary\":\"缝纫机基准摘要\"}\n");
    }
    let p = dir.join(format!("{sid}.jsonl"));
    let mut f = fs::File::create(&p).unwrap();
    f.write_all(body.as_bytes()).unwrap();
    (events, body.len() as u64)
}

struct Dataset {
    root: PathBuf,
    files: usize,
    bytes: u64,
}

/// 生成 n 个会话、10 个项目；每第 10 个带搜索标记；附加巨型会话（仅 dataset≥500）。
fn generate(n: usize) -> Dataset {
    let root = std::env::temp_dir().join(format!(
        "resession-bench-{n}-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    let mut files = 0usize;
    let mut bytes = 0u64;
    for i in 0..n {
        let proj = root.join(format!("D--bench-proj-{:02}", i % 10));
        fs::create_dir_all(&proj).unwrap();
        let sid = format!("b{n:04}x{i:06}");
        let marker = i % 10 == 3;
        let events = 40 + (i * 7) % 80;
        let (ev, b) = write_session(&proj, &sid, events, marker);
        files += 1;
        bytes += b;
        let _ = ev;
    }
    // 巨型会话：dataset ≥ 500 时附加（虚拟滚动决策的数据点）
    if n >= 500 {
        let proj = root.join("D--bench-big");
        fs::create_dir_all(&proj).unwrap();
        let (_, b) = write_session(&proj, "big0000-0000-0000-0000-000000000000", 20_000, false);
        files += 1;
        bytes += b;
    }
    Dataset { root, files, bytes }
}

fn fmt_dur(d: Duration) -> String {
    let ms = d.as_secs_f64() * 1000.0;
    if ms < 1000.0 {
        format!("{ms:.0}ms")
    } else {
        format!("{:.2}s", ms / 1000.0)
    }
}

fn fmt_mb(b: u64) -> String {
    format!("{:.1}MB", b as f64 / 1024.0 / 1024.0)
}

fn bench_dataset(n: usize) -> Vec<String> {
    let ds = generate(n);
    let provider = ClaudeProvider::with_root(ds.root.clone());

    // 冷扫描：清缓存后第一次（全量解析）
    ClaudeProvider::clear_caches();
    let t = Instant::now();
    let sessions = provider.scan().expect("scan");
    let cold_scan = t.elapsed();

    // 热扫描：缓存全命中（纯 stat），取 3 次中位数
    let mut warm: Vec<Duration> = Vec::new();
    for _ in 0..3 {
        let t = Instant::now();
        provider.scan().unwrap();
        warm.push(t.elapsed());
    }
    warm.sort();
    let warm_scan = warm[1];

    // 冷搜索：首次（需构建全部可搜索文本缓存）
    let t = Instant::now();
    let hits = provider.search("缝纫机").unwrap();
    let cold_search = t.elapsed();

    // 热搜索：文本缓存全命中，3 次取中位数
    let mut search_warm: Vec<Duration> = Vec::new();
    for _ in 0..3 {
        let t = Instant::now();
        provider.search("缝纫机").unwrap();
        search_warm.push(t.elapsed());
    }
    search_warm.sort();
    let warm_search = search_warm[1];

    // 最大转录加载（用户实际打开最大会话的耗时）
    let mut big = sessions.clone();
    big.sort_by_key(|s| std::cmp::Reverse(fs::metadata(&s.source_file).map(|m| m.len()).unwrap_or(0)));
    let big_load = big
        .first()
        .map(|meta| {
            let bytes = fs::metadata(&meta.source_file).map(|m| m.len()).unwrap_or(0);
            let t = Instant::now();
            let ev = provider.load_transcript(meta).expect("load");
            (t.elapsed(), bytes, ev.len())
        })
        .map(|(d, bytes, events)| format!("{}（{}，{} 事件）", fmt_dur(d), fmt_mb(bytes), events))
        .unwrap_or_else(|| "—".into());

    let _ = fs::remove_dir_all(&ds.root);
    let _ = hits;

    vec![format!(
        "{n} 会话 | {} | {} | {} | {} | {} | {} | {}",
        ds.files,
        fmt_mb(ds.bytes),
        fmt_dur(cold_scan),
        fmt_dur(warm_scan),
        fmt_dur(cold_search),
        fmt_dur(warm_search),
        big_load
    )]
}
