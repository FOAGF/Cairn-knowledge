//! Full-scale test data for `make measure` (NFR-PERF-4, NFR-PERF-6, D-51):
//!
//! ```text
//! cargo run --release -p cairn-core --example measure_data -- <data-dir> [--pages N] [--tasks N]
//!                                                              [--mock-port P] [--seed S]
//! ```
//!
//! Makes a data folder the app can open (`CAIRN_DATA_DIR=<data-dir>`) with 10,000 pages and 20,000
//! tasks by default, some of them Chinese, about 20 categories, groups, tags, task labels, task
//! comments, a glossary and task-page links, all through the core's own save paths.
//!
//! **Search vectors are generated**, not embedded: a random unit vector for every piece, recorded
//! in `vector_meta` with the model id and dims the app uses (`src-tauri/src/sidecar.rs`:
//! `EMBED_MODEL_ID`, `EMBED_DIMS`), so the indexer finds nothing to embed. The scan's speed doesn't
//! depend on what the vectors say; the real model still embeds each query.
//!
//! Settings: daily backups off, and a "Mock LLM" provider (kind `local`, no key) with one model as
//! the Assistant's default, pointing at `scripts/mock_llm.py` on `--mock-port` (default 5299).
//!
//! The last stdout line is a JSON summary; progress goes to stderr.

use std::path::PathBuf;
use std::time::Instant;

use cairn_core::Db;
use cairn_core::error::CoreError;
use cairn_core::glossary::{self, NewGlossaryEntry};
use cairn_core::knowledge::{self, NewGroup, NewPage};
use cairn_core::categories::{self, NewCategory, CategoryColor};
use cairn_core::providers::{self, AuthMode, NewModel, NewProvider, ProviderKind, RequestFormat};
use cairn_core::secrets::MemoryStore;
use cairn_core::settings::{self, SettingsPatch};
use cairn_core::tasks::{self, NewTask, Priority, TaskStatus, TaskType};
use cairn_core::vectors::{self, IndexModel};

/// Must equal `EMBED_MODEL_ID` and `EMBED_DIMS` in `src-tauri/src/sidecar.rs`. If they drift, the
/// app empties the vectors at start and `make measure` notices (the index is no longer ready).
const MODEL_ID: &str = "embeddinggemma-300m-q4";
const DIMS: usize = 256;

/// Words for English text. `scripts/measure/measure.py` takes its queries from this list.
const WORDS: &[&str] = &[
    "pipeline",
    "sensor",
    "data",
    "partition",
    "query",
    "index",
    "search",
    "report",
    "deploy",
    "release",
    "cluster",
    "storage",
    "latency",
    "model",
    "vector",
    "schema",
    "migration",
    "review",
    "backlog",
    "sprint",
    "design",
    "cache",
    "network",
    "buffer",
    "thread",
    "pump",
    "calibration",
    "maintenance",
    "inspection",
    "hose",
    "valve",
    "pressure",
    "safety",
    "isolation",
    "procedure",
    "handover",
    "shift",
    "checklist",
    "supplier",
    "invoice",
    "budget",
    "forecast",
    "schedule",
    "vehicle",
    "service",
    "battery",
    "training",
    "incident",
    "audit",
    "permit",
    "drainage",
    "survey",
    "sample",
    "result",
    "threshold",
    "alarm",
    "firmware",
    "upgrade",
    "backup",
    "restore",
    "capacity",
    "workload",
    "meeting",
    "decision",
    "owner",
    "risk",
    "mitigation",
    "contract",
    "delivery",
    "stock",
    "order",
    "spare",
    "tool",
    "workshop",
    "weather",
    "forecasting",
];

/// Han text for Chinese titles and paragraphs.
const HAN: &[&str] = &[
    "数据管道设计",
    "分区查询速度",
    "索引搜索报告",
    "部署发布计划",
    "集群存储延迟",
    "模型向量迁移",
    "泵的校准和维护",
    "安全隔离程序",
    "交接班记录",
    "供应商发票核对",
    "预算和预测",
    "车辆保养计划",
    "培训安排",
    "事故调查结果",
    "许可证审核",
    "排水系统检查",
    "样品测试结果",
    "报警阈值设置",
    "固件升级步骤",
    "备份和恢复",
    "工作量评估",
    "会议决定事项",
    "风险和缓解措施",
    "合同交付日期",
    "备件库存订单",
    "工具和车间",
    "天气预报影响",
];

const CATEGORIES: &[(&str, &str)] = &[
    ("Pump Maintenance", "PMP"),
    ("Fleet Operations", "FLT"),
    ("Data Platform", "DAT"),
    ("Safety Program", "SAF"),
    ("Supplier Management", "SUP"),
    ("Site Survey", "SRV"),
    ("Training Hub", "TRN"),
    ("Field Reports", "FLD"),
    ("Budget Planning", "BUD"),
    ("Release Engineering", "REL"),
    ("Quality Audit", "AUD"),
    ("Spare Parts", "SPR"),
    ("Workshop", "WSH"),
    ("Permits", "PRM"),
    ("Drainage", "DRN"),
    ("Firmware", "FRM"),
    ("Contracts", "CON"),
    ("Weather Watch", "WTH"),
    ("Handover Notes", "HND"),
];

/// Small, fast, repeatable random numbers (xorshift64*).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        ((self.next() >> 11) % n as u64) as usize
    }
    /// 0..1
    fn unit(&mut self) -> f32 {
        ((self.next() >> 40) as f32) / (1u64 << 24) as f32
    }
    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }
}

fn sentence(rng: &mut Rng, words: usize) -> String {
    let mut out = String::new();
    for i in 0..words {
        if i > 0 {
            out.push(' ');
        }
        let w = *rng.pick(WORDS);
        if i == 0 {
            let mut c = w.chars();
            out.extend(c.next().map(|f| f.to_ascii_uppercase()));
            out.push_str(c.as_str());
        } else {
            out.push_str(w);
        }
        // A rare token now and then, so some queries match only a few items.
        if rng.below(12) == 0 {
            out.push_str(&format!(" w{}", rng.below(3000)));
        }
    }
    out.push('.');
    out
}

fn han_sentence(rng: &mut Rng, phrases: usize) -> String {
    let mut out = String::new();
    for _ in 0..phrases {
        out.push_str(rng.pick(HAN));
        out.push('，');
    }
    out.pop();
    out.push('。');
    out
}

fn paragraph(rng: &mut Rng, chinese: bool) -> String {
    let n = 3 + rng.below(5);
    (0..n)
        .map(|_| {
            let (phrases, words) = (2 + rng.below(3), 8 + rng.below(12));
            if chinese && rng.below(2) == 0 {
                han_sentence(rng, phrases)
            } else {
                sentence(rng, words)
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// 3 to 6 sections of two or three paragraphs, so a page has several pieces.
fn page_body(rng: &mut Rng, chinese: bool) -> String {
    let mut out = String::new();
    for s in 0..3 + rng.below(4) {
        out.push_str(&format!(
            "## {} {}\n\n",
            sentence(rng, 2).trim_end_matches('.'),
            s + 1
        ));
        for _ in 0..2 + rng.below(2) {
            out.push_str(&paragraph(rng, chinese));
            out.push_str("\n\n");
        }
    }
    out
}

struct Args {
    dir: PathBuf,
    pages: usize,
    tasks: usize,
    mock_port: u16,
    seed: u64,
}

fn parse_args() -> Args {
    let mut args = Args {
        dir: PathBuf::new(),
        pages: 10_000,
        tasks: 20_000,
        mock_port: 5299,
        seed: 20_261_016,
    };
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        let mut value = |what: &str| {
            it.next()
                .unwrap_or_else(|| usage(&format!("{what} needs a value")))
        };
        match a.as_str() {
            "--pages" => {
                args.pages = value("--pages")
                    .parse()
                    .unwrap_or_else(|_| usage("--pages: a number"))
            }
            "--tasks" => {
                args.tasks = value("--tasks")
                    .parse()
                    .unwrap_or_else(|_| usage("--tasks: a number"))
            }
            "--mock-port" => {
                args.mock_port = value("--mock-port")
                    .parse()
                    .unwrap_or_else(|_| usage("--mock-port: a number"))
            }
            "--seed" => {
                args.seed = value("--seed")
                    .parse()
                    .unwrap_or_else(|_| usage("--seed: a number"))
            }
            s if !s.starts_with("--") && args.dir.as_os_str().is_empty() => args.dir = s.into(),
            other => usage(&format!("unknown argument {other}")),
        }
    }
    if args.dir.as_os_str().is_empty() {
        usage("give the data folder");
    }
    args
}

fn usage(why: &str) -> ! {
    eprintln!("measure_data: {why}");
    eprintln!("usage: measure_data <data-dir> [--pages N] [--tasks N] [--mock-port P] [--seed S]");
    std::process::exit(2);
}

fn main() -> cairn_core::Result<()> {
    let args = parse_args();
    if args.dir.join("cairn.db").exists() {
        eprintln!(
            "measure_data: {} already holds data; use an empty folder",
            args.dir.display()
        );
        std::process::exit(2);
    }
    let started = Instant::now();
    let mut rng = Rng(args.seed | 1);
    let mut db = Db::open_at(&args.dir)?;
    // D-72: knowledge in the database, so the app doesn't ask (the full-screen mode choice).
    cairn_core::knowledge::mode::choose_mode(
        &mut db,
        cairn_core::settings::KnowledgeMode::Internal,
        None,
    )?;

    // Settings: no daily backup during measuring, and the mock LLM as the Assistant's model.
    settings::update(
        &mut db,
        SettingsPatch {
            user_name: Some("Measure".into()),
            backup_enabled: Some(false),
            ..Default::default()
        },
    )?;
    let provider = providers::create_provider(
        &mut db,
        &MemoryStore::default(),
        NewProvider {
            name: "Mock LLM".into(),
            kind: ProviderKind::Local,
            endpoint: Some(format!("http://127.0.0.1:{}/v1", args.mock_port)),
            api_version: None,
            auth: AuthMode::None,
        },
    )?;
    let model = providers::create_model(
        &mut db,
        NewModel {
            provider_id: provider.id,
            name: "mock-model".into(),
            label: Some("Mock".into()),
            format: RequestFormat::ChatCompletions,
            vision: None,
            effort: None,
            context_size: None,
            web_search: None,
        },
    )?;
    settings::update(
        &mut db,
        SettingsPatch {
            default_model_id: Some(model.id),
            ..Default::default()
        },
    )?;

    // Categories (the built-in General category stays), groups, glossary.
    let mut category_ids = vec!["p-general".to_string()];
    for (i, (name, key)) in CATEGORIES.iter().enumerate() {
        let p = categories::create(
            db.conn(),
            NewCategory {
                name: (*name).into(),
                key: (*key).into(),
                description: format!("{} work, generated for measuring.", name.to_lowercase()),
                color: CategoryColor::preset(CategoryColor::PRESETS[i % CategoryColor::PRESETS.len()]),
            },
        )?;
        category_ids.push(p.id);
    }
    let mut groups: Vec<(String, String)> = Vec::new(); // (category id, group id)
    for p in &category_ids {
        for g in 0..3 + rng.below(4) {
            let name = format!("{} {}", sentence(&mut rng, 2).trim_end_matches('.'), g + 1);
            let group = knowledge::create_group(
                &mut db,
                NewGroup {
                    category_id: p.clone(),
                    name,
                    description: Some(sentence(&mut rng, 6)),
                    parent_id: None,
                },
            )?;
            groups.push((p.clone(), group.id));
        }
    }
    for i in 0..60 {
        let global = i % 3 == 0;
        let entry = NewGlossaryEntry {
            category_id: (!global).then(|| rng.pick(&category_ids).clone()),
            term: format!("{} {}", rng.pick(WORDS), rng.pick(WORDS)),
            other_names: Some(vec![format!("{}-{}", rng.pick(WORDS), i)]),
            description: Some(sentence(&mut rng, 12)),
        };
        // A repeated random term is skipped.
        match glossary::create(&mut db, entry) {
            Ok(_) | Err(CoreError::Conflict(_)) => {}
            Err(e) => return Err(e),
        }
    }
    eprintln!(
        "categories, groups, glossary: {:.1} s",
        started.elapsed().as_secs_f32()
    );

    let tags: Vec<String> = (0..40)
        .map(|i| format!("{}-{}", rng.pick(WORDS), i % 7))
        .collect();
    let labels: Vec<String> = (0..25)
        .map(|i| format!("{}{}", rng.pick(WORDS), i % 5))
        .collect();

    // Pages: about one in six has Chinese text, one in twenty a Chinese title.
    let mut page_ids = Vec::with_capacity(args.pages);
    for i in 0..args.pages {
        let category = rng.pick(&category_ids).clone();
        let in_group: Vec<&(String, String)> = groups.iter().filter(|g| g.0 == category).collect();
        let chinese = rng.below(6) == 0;
        let title = if rng.below(20) == 0 {
            format!("{} {}", rng.pick(HAN), i)
        } else {
            format!("{} {}", sentence(&mut rng, 3).trim_end_matches('.'), i)
        };
        let page_tags: Vec<String> = (0..rng.below(4)).map(|_| rng.pick(&tags).clone()).collect();
        let page = knowledge::create_page(
            &mut db,
            NewPage {
                group_id: (rng.below(10) < 7 && !in_group.is_empty())
                    .then(|| in_group[rng.below(in_group.len())].1.clone()),
                tags: Some(page_tags),
                ..NewPage {
                    category_id: category,
                    title,
                    body: page_body(&mut rng, chinese),
                    ..Default::default()
                }
            },
        )?;
        page_ids.push(page.id);
        if (i + 1) % 1000 == 0 {
            eprintln!("pages {}: {:.0} s", i + 1, started.elapsed().as_secs_f32());
        }
    }

    // Tasks, with comments on about a third and labels on some.
    let today = chrono::Local::now().date_naive();
    let mut task_ids = Vec::with_capacity(args.tasks);
    let mut comments = 0usize;
    for i in 0..args.tasks {
        let chinese = rng.below(7) == 0;
        let status = match rng.below(100) {
            0..=44 => TaskStatus::Todo,
            45..=56 => TaskStatus::InProgress,
            57..=60 => TaskStatus::Blocked,
            61..=94 => TaskStatus::Done,
            _ => TaskStatus::WontDo,
        };
        let due = (rng.below(3) == 0).then(|| {
            (today + chrono::Duration::days(rng.below(60) as i64 - 20))
                .format("%Y-%m-%d")
                .to_string()
        });
        let words = 4 + rng.below(4);
        let task = tasks::create(
            &mut db,
            NewTask {
                category_id: rng.pick(&category_ids).clone(),
                title: if chinese {
                    format!("{}{}", rng.pick(HAN), sentence(&mut rng, 2))
                } else {
                    sentence(&mut rng, words)
                },
                task_type: Some(*rng.pick(&[TaskType::Bug, TaskType::Feature, TaskType::General])),
                description: Some(paragraph(&mut rng, chinese)),
                status: Some(status),
                priority: Some(*rng.pick(&[Priority::High, Priority::Medium, Priority::Low])),
                labels: (rng.below(3) == 0).then(|| vec![rng.pick(&labels).clone()]),
                due_date: due,
            },
        )?;
        if rng.below(3) == 0 {
            tasks::add_comment(&mut db, &task.id, &paragraph(&mut rng, chinese))?;
            comments += 1;
        }
        task_ids.push(task.id);
        if (i + 1) % 2000 == 0 {
            eprintln!("tasks {}: {:.0} s", i + 1, started.elapsed().as_secs_f32());
        }
    }

    // Task-page links: about one task in ten.
    let mut links = 0usize;
    if !page_ids.is_empty() {
        for _ in 0..task_ids.len() / 10 {
            let (t, p) = (rng.pick(&task_ids).clone(), rng.pick(&page_ids).clone());
            if knowledge::link_task_page(&mut db, &t, &p).is_ok() {
                links += 1;
            }
        }
    }

    // Generated vectors for every piece, then the model record, so the indexer has nothing to do.
    let conn = db.conn();
    let waiting: Vec<i64> = conn
        .prepare("SELECT id FROM chunks WHERE vector IS NULL")?
        .query_map([], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    let tx = conn.unchecked_transaction()?;
    {
        let mut set = tx.prepare("UPDATE chunks SET vector = ?1 WHERE id = ?2")?;
        for id in &waiting {
            let mut v: Vec<f32> = (0..DIMS).map(|_| rng.unit() * 2.0 - 1.0).collect();
            let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-6);
            v.iter_mut().for_each(|x| *x /= norm);
            set.execute(rusqlite::params![vectors::to_blob(&v), id])?;
        }
    }
    tx.commit()?;
    vectors::ensure_model(
        conn,
        &IndexModel {
            id: MODEL_ID.into(),
            dims: DIMS as u32,
        },
    )?;
    let status = vectors::status(conn)?;
    eprintln!(
        "vectors for {} pieces, index {}/{} pieces ready",
        waiting.len(),
        status.chunks_ready,
        status.chunks
    );

    println!(
        "{{\"dir\":{:?},\"pages\":{},\"tasks\":{},\"categories\":{},\"groups\":{},\"taskComments\":{},\"taskPageLinks\":{},\"chunks\":{},\"chunksReady\":{},\"model\":{:?},\"dims\":{},\"seconds\":{:.1}}}",
        args.dir.display().to_string(),
        page_ids.len(),
        task_ids.len(),
        category_ids.len(),
        groups.len(),
        comments,
        links,
        status.chunks,
        status.chunks_ready,
        MODEL_ID,
        DIMS,
        started.elapsed().as_secs_f32()
    );
    Ok(())
}
