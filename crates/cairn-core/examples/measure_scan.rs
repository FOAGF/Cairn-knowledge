//! D-72 scale check: the first scan of a knowledge folder in external mode, through the same three
//! steps the app uses (`src-tauri/src/knowledge_folder.rs`): `scan::prepare` and `apply_some`
//! each hold the database (the app's lock), reading the folder doesn't. Reports the time of each
//! step and how long each batch held the database.
//!
//!   python3 -I scripts/probes/ext-gen-folder.py /tmp/x-big 10000
//!   cargo run --release -p cairn-core --example measure_scan -- /tmp/x-data /tmp/x-big
//!
//! The data folder must be new (or empty); the knowledge folder is only read.

use std::path::Path;
use std::time::{Duration, Instant};

use cairn_core::Db;
use cairn_core::knowledge::external::scan::{self, Scope};
use cairn_core::knowledge::mode;
use cairn_core::settings::KnowledgeMode;

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

fn main() -> cairn_core::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("usage: measure_scan <new data folder> <knowledge folder>");
        std::process::exit(2);
    }
    let (data, root) = (Path::new(&args[1]), &args[2]);
    std::fs::create_dir_all(data)?;
    let mut db = Db::open_at(data)?;
    mode::choose_mode(&mut db, KnowledgeMode::External, Some(root))?;

    let start = Instant::now();
    let t = Instant::now();
    let job = scan::prepare(db.conn(), Scope::All)?;
    let prepare = t.elapsed();
    let t = Instant::now();
    let mut plan = job.read(&mut |_| {})?;
    let read = t.elapsed();
    let total = plan.total();
    let mut holds: Vec<Duration> = Vec::new();
    let mut items: Vec<usize> = Vec::new();
    loop {
        let t = Instant::now();
        let n = plan.apply_some(&mut db, scan::BATCH)?;
        let held = t.elapsed();
        if n == 0 {
            break;
        }
        holds.push(held);
        items.push(n);
    }
    let all = start.elapsed();
    let mut sorted = holds.clone();
    sorted.sort();
    let longest = holds.iter().enumerate().max_by_key(|(_, d)| **d).map(|(i, d)| (i, *d));
    println!("items to apply: {total}");
    println!("prepare (holds the database): {:.1} ms", ms(prepare));
    println!("read the folder (no lock): {:.0} ms", ms(read));
    println!(
        "apply: {} batches, {:.0} ms in all; hold per batch: median {:.1} ms, p95 {:.1} ms, longest {:.1} ms (batch {} of {} items)",
        holds.len(),
        ms(holds.iter().sum()),
        ms(sorted[sorted.len() / 2]),
        ms(sorted[sorted.len() * 95 / 100]),
        ms(longest.map_or(Duration::ZERO, |l| l.1)),
        longest.map_or(0, |l| l.0 + 1),
        longest.map_or(0, |l| items[l.0]),
    );
    println!(
        "items per batch: min {}, median {}, max {}",
        items.iter().min().unwrap_or(&0),
        { let mut s = items.clone(); s.sort(); s[s.len() / 2] },
        items.iter().max().unwrap_or(&0)
    );
    // The trend: items per batch in tenths of the run (a falling number means each page costs more
    // as the index grows).
    let tenth = (items.len() / 10).max(1);
    let trend: Vec<String> = items
        .chunks(tenth)
        .zip(holds.chunks(tenth))
        .map(|(n, h)| format!("{:.2}", ms(h.iter().sum()) / n.iter().sum::<usize>() as f64))
        .collect();
    println!("ms per item, by tenth of the run: {}", trend.join(" "));
    println!("first scan in all: {:.2} s", all.as_secs_f64());
    println!("summary: {:?}", plan.summary());
    Ok(())
}
