use std::hint::black_box;
use std::time::{Duration, UNIX_EPOCH};

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use duet_core::folder::entry::{Entry, EntryKind};
use duet_core::folder::tree::build_tree;

fn side(files: u32, grow_every: u32) -> Vec<Entry> {
    let modified = Some(UNIX_EPOCH + Duration::from_secs(1_700_000_000));
    let mut entries = Vec::new();
    for d in 0..files / 100 {
        entries.push(Entry {
            rel: format!("d{d:04}"),
            kind: EntryKind::Dir,
            len: 0,
            modified,
            error: None,
            link_target: None,
        });
    }
    for i in 0..files {
        let len = if grow_every > 0 && i % grow_every == 0 {
            170
        } else {
            160
        };
        entries.push(Entry {
            rel: format!("d{:04}/f{i:06}.txt", i / 100),
            kind: EntryKind::File,
            len,
            modified,
            error: None,
            link_target: None,
        });
    }
    entries
}

fn bench(c: &mut Criterion) {
    let (left, right) = (side(100_000, 0), side(100_000, 100));
    c.bench_function("build_tree, 100k files per side", |b| {
        b.iter_batched(
            || (left.clone(), right.clone()),
            |(l, r)| black_box(build_tree(l, r, true)),
            BatchSize::LargeInput,
        )
    });
}

criterion_group!(benches, bench);
criterion_main!(benches);
