use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use duet_core::cancel::CancelToken;
use duet_core::text::diff::{DiffInput, DiffOptions, diff_lines};
use duet_core::text::lines::LineIndex;

/// About 10 MB per side, 1% of lines replaced.
fn make_pair() -> (String, String) {
    let (mut left, mut right) = (String::new(), String::new());
    for i in 0..190_000u64 {
        let line = format!(
            "{i:010} lorem ipsum dolor sit amet consectetur {}\n",
            i * 7919 % 1000
        );
        left.push_str(&line);
        if i % 100 == 0 {
            right.push_str(&format!("{i:010} CHANGED\n"));
        } else {
            right.push_str(&line);
        }
    }
    (left, right)
}

fn bench(c: &mut Criterion) {
    let (left, right) = make_pair();
    c.bench_function("index + diff, 10 MB, 1% changed", |b| {
        b.iter(|| {
            let (li, ri) = (
                LineIndex::new(left.as_bytes()),
                LineIndex::new(right.as_bytes()),
            );
            black_box(
                diff_lines(
                    DiffInput {
                        bytes: left.as_bytes(),
                        lines: &li,
                    },
                    DiffInput {
                        bytes: right.as_bytes(),
                        lines: &ri,
                    },
                    DiffOptions::default(),
                    &CancelToken::new(),
                )
                .unwrap(),
            )
        })
    });
}

criterion_group!(benches, bench);
criterion_main!(benches);
