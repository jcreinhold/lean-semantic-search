//! Large-N benchmark: a query over a persisted corpus must keep its Rust-side
//! resident set proportional to the work of the query — the anchors planned and
//! the postings touched — never to the corpus size.
//!
//! This binary installs `dhat` as its global allocator and reports peak heap
//! (`max_bytes`) while serving one query against corpora of two sizes. `SQLite`'s
//! own page cache and scratch buffers are C-allocated and bounded by its fixed
//! cache, so they do not appear here; what `dhat` measures is exactly the query's
//! Rust working set. Marked `#[ignore]` so the default suite stays fast; run with:
//!
//! ```sh
//! cargo test -p lean-semantic-search-store --test benchmark -- --ignored --nocapture
//! ```

mod common;

use std::time::{Duration, Instant};

use common::{declaration, fingerprints, role, temp_path};
use lean_semantic_search_contract::DeclarationFeatureRow;
use lean_semantic_search_retrieval::{Anchor, retrieve_across};
use lean_semantic_search_store::{Ingest, Store, StoreBuilder};

#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

/// A fixed cohort shares the anchor's keys; everyone else is unique. The cohort
/// size does not grow with N, so the postings a query touches stay constant.
const COHORT: usize = 64;

/// Generous ceiling on the query's Rust working set, in bytes. The query touches
/// at most the cohort's postings and the anchor's plan; this is far above that
/// and far below anything proportional to a 100k+ row corpus.
const HEAP_BOUND: usize = 4 * 1024 * 1024;

fn synthetic_row(index: usize) -> DeclarationFeatureRow {
    let id = format!("decl-{index:09}");
    if index < COHORT {
        declaration(
            &id,
            fingerprints("shared"),
            vec![role("conclusion_const", "rare-role", "Rare")],
        )
    } else {
        declaration(
            &id,
            fingerprints(&format!("u{index}")),
            vec![role("conclusion_const", &format!("rk-{index}"), "Solo")],
        )
    }
}

fn anchor_row() -> DeclarationFeatureRow {
    declaration(
        "anchor",
        fingerprints("shared"),
        vec![role("conclusion_const", "rare-role", "Rare")],
    )
}

struct Measurement {
    documents: usize,
    build: Duration,
    query: Duration,
    peak_heap: usize,
    candidates: usize,
}

/// Build a corpus of `documents` rows streaming (one row resident at a time),
/// then measure the peak Rust heap and latency of a single query against it.
fn measure(documents: usize) -> Result<Measurement, String> {
    let path = temp_path(&format!("bench-{documents}"));

    let build_start = Instant::now();
    let mut builder = StoreBuilder::create(&path, "bench").map_err(|e| e.to_string())?;
    for index in 0..documents {
        let row = synthetic_row(index);
        builder
            .accept(Ingest::Declaration(row.declaration_id.clone()))
            .map_err(|e| e.to_string())?;
        builder.accept(Ingest::Feature(row)).map_err(|e| e.to_string())?;
    }
    let published = builder.publish().map_err(|e| e.to_string())?;
    let build = build_start.elapsed();

    let anchor = Anchor::from_declaration(&anchor_row());

    // Start measuring at open, so the open store's retained footprint and the
    // query's allocations are both counted; the build's transients are not.
    let profiler = dhat::Profiler::builder().testing().build();
    let store = Store::open(&published).map_err(|e| e.to_string())?;
    let query_start = Instant::now();
    let retrieval = retrieve_across(&[&store], &anchor, 20);
    let query = query_start.elapsed();
    let peak_heap = dhat::HeapStats::get().max_bytes;
    let candidates = retrieval.candidates.len();
    drop(profiler);

    std::fs::remove_file(&path).ok();
    Ok(Measurement {
        documents,
        build,
        query,
        peak_heap,
        candidates,
    })
}

#[test]
#[ignore = "large-N benchmark; run explicitly with --ignored --nocapture"]
fn resident_set_is_bounded_by_the_query_not_the_corpus() -> Result<(), String> {
    let small = measure(100_000)?;
    let large = measure(200_000)?;

    for m in [&small, &large] {
        println!(
            "N={:>7}  build={:>8.2?}  query={:>10.2?}  peak_heap={:>9} bytes  candidates={}",
            m.documents, m.build, m.query, m.peak_heap, m.candidates
        );
    }

    // The anchor matches the same fixed cohort at both sizes.
    assert_eq!(
        small.candidates, large.candidates,
        "query should touch the same cohort at both sizes"
    );

    // (a) The query's heap stays under a fixed bound at both sizes.
    assert!(
        small.peak_heap < HEAP_BOUND,
        "100k peak heap {} exceeded bound {HEAP_BOUND}",
        small.peak_heap
    );
    assert!(
        large.peak_heap < HEAP_BOUND,
        "200k peak heap {} exceeded bound {HEAP_BOUND}",
        large.peak_heap
    );

    // (b) Doubling the corpus does not roughly double the query's heap: it stays
    //     within a small constant, i.e. visibly sub-linear in corpus size.
    let ratio = large.peak_heap as f64 / small.peak_heap.max(1) as f64;
    assert!(
        ratio < 1.5,
        "peak heap scaled with corpus size: 200k/100k ratio {ratio:.2}"
    );
    Ok(())
}
