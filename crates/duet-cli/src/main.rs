//! Headless compare: scripting, golden tests and benchmarks. Exit 0 = identical,
//! 1 = different, 2 = error.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

use clap::{Parser, ValueEnum};
use duet_core::cancel::CancelToken;
use duet_core::folder::content::files_equal;
use duet_core::folder::entry::Filters;
use duet_core::folder::run::{CompareMode, FolderOptions, FolderProgress, compare_folders};
use duet_core::folder::tree::{QuickRule, Status};
use duet_core::text::diff::{DiffInput, DiffOptions, WhitespaceMode, diff_lines};
use duet_core::text::document::{LoadError, TextDocument};

mod report;

const AFTER_HELP: &str = r#"Exit codes: 0 = identical, 1 = different, 2 = error.

Folder rows (plain output lists only rows that are not identical):
  ! different    < left only    > right only    ? error (cannot read)    ~ pending

Text hunks use unified-diff headers: @@ -left_start,count +right_start,count @@.
Hunks marked "line endings only" spell out each line's terminator: \r\n, \n, \r or (no newline)."#;

#[derive(Parser, Debug)]
#[command(
    name = "duet-cli",
    about = "Compare two files or two folders (offline, headless)",
    after_help = AFTER_HELP
)]
struct Args {
    /// Left file or folder.
    left: PathBuf,
    /// Right file or folder (same kind as LEFT).
    right: PathBuf,
    /// Byte-compare same-size files instead of trusting size + modification time.
    #[arg(long)]
    content: bool,
    /// Text: ignore whitespace at line edges, or all whitespace.
    #[arg(long, value_enum, default_value_t = Ws::None)]
    ignore_ws: Ws,
    /// Text: ignore upper/lower case differences.
    #[arg(long)]
    ignore_case: bool,
    /// Text: Ignore line-ending differences (CRLF, LF, CR, missing final newline).
    #[arg(long)]
    ignore_eol: bool,
    /// Folders: only compare files matching this glob (name or relative path); repeatable.
    #[arg(long = "include")]
    include: Vec<String>,
    /// Folders: skip files and whole folders matching this glob; repeatable.
    #[arg(long = "exclude")]
    exclude: Vec<String>,
    /// Folders: pair names case-sensitively (default pairs "A.txt" with "a.txt").
    #[arg(long)]
    case_sensitive: bool,
    /// Folders (quick mode): seconds two modification times may differ and still match.
    #[arg(long, default_value_t = 2)]
    time_tolerance: u64,
    /// Folders (quick mode): also treat times exactly one hour apart as matching.
    #[arg(long)]
    ignore_dst: bool,
    /// Print machine-readable JSON instead of plain text.
    #[arg(long)]
    json: bool,
    /// Print only a one-line summary.
    #[arg(long)]
    quiet: bool,
    /// Print phase timings to stderr.
    #[arg(long)]
    timings: bool,
}

#[derive(Copy, Clone, Debug, ValueEnum)]
enum Ws {
    None,
    Edges,
    All,
}

impl From<Ws> for WhitespaceMode {
    fn from(ws: Ws) -> Self {
        match ws {
            Ws::None => WhitespaceMode::None,
            Ws::Edges => WhitespaceMode::Edges,
            Ws::All => WhitespaceMode::All,
        }
    }
}

enum Outcome {
    Identical,
    Different,
}

fn main() -> ExitCode {
    let args = Args::parse();
    let start = Instant::now();
    let result = run(&args);
    if args.timings {
        eprintln!("timing: total_ms={}", start.elapsed().as_millis());
    }
    match result {
        Ok(Outcome::Identical) => ExitCode::from(0),
        Ok(Outcome::Different) => ExitCode::from(1),
        Err(message) => {
            eprintln!("duet-cli: {message}");
            ExitCode::from(2)
        }
    }
}

fn run(args: &Args) -> Result<Outcome, String> {
    let meta = |p: &PathBuf| std::fs::metadata(p).map_err(|e| format!("{}: {e}", p.display()));
    match (meta(&args.left)?.is_dir(), meta(&args.right)?.is_dir()) {
        (true, true) => run_folder(args),
        (false, false) => run_text(args),
        _ => Err("cannot compare a file with a folder".into()),
    }
}

fn run_text(args: &Args) -> Result<Outcome, String> {
    let cancel = CancelToken::new();
    let loaded = Instant::now();
    // Open both sides at once: loading is mostly page-ins and line indexing.
    let opened = std::thread::scope(|scope| {
        let right = scope.spawn(|| TextDocument::open(&args.right, &cancel));
        let left = TextDocument::open(&args.left, &cancel);
        (left, right.join().expect("loader thread panicked"))
    });
    let (left, right) = match opened {
        (Ok(l), Ok(r)) => (l, r),
        (Err(LoadError::Binary { .. }), _) | (_, Err(LoadError::Binary { .. })) => {
            return run_binary(args, &cancel);
        }
        (Err(e), _) | (_, Err(e)) => return Err(e.to_string()),
    };
    let load_ms = loaded.elapsed().as_millis();
    let opts = DiffOptions {
        whitespace: args.ignore_ws.into(),
        ignore_case: args.ignore_case,
        ignore_eol: args.ignore_eol,
    };
    let diffed = Instant::now();
    let diff = diff_lines(
        DiffInput {
            bytes: left.bytes(),
            lines: left.lines(),
        },
        DiffInput {
            bytes: right.bytes(),
            lines: right.lines(),
        },
        opts,
        &cancel,
    )
    .map_err(|e| e.to_string())?;
    if args.timings {
        eprintln!(
            "timing: load_ms={load_ms} diff_ms={}",
            diffed.elapsed().as_millis()
        );
    }
    let bytes_differ = diff.is_identical() && decoding_hides_bytes(&left, &right) && {
        !files_equal(&args.left, &args.right, &cancel).map_err(|e| e.to_string())?
    };
    let identical = diff.is_identical() && !bytes_differ;
    if args.quiet {
        println!(
            "{}",
            report::summary(identical, diff.hunks.len(), bytes_differ)
        );
    } else if args.json {
        println!("{}", report::text_json(&left, &right, &diff, bytes_differ));
    } else {
        print!("{}", report::text_plain(&left, &right, &diff));
        if bytes_differ {
            println!("{}", report::BYTES_DIFFER);
        }
    }
    Ok(if identical {
        Outcome::Identical
    } else {
        Outcome::Different
    })
}

/// Equal decoded text proves equal bytes only when both sides decoded losslessly with the same
/// encoding and BOM; otherwise undecodable bytes (all U+FFFD) or the BOM can still differ.
fn decoding_hides_bytes(left: &TextDocument, right: &TextDocument) -> bool {
    left.is_lossy() || right.is_lossy() || left.encoding() != right.encoding()
}

fn run_binary(args: &Args, cancel: &CancelToken) -> Result<Outcome, String> {
    let equal = files_equal(&args.left, &args.right, cancel).map_err(|e| e.to_string())?;
    println!(
        "{}",
        if equal {
            "binary files identical"
        } else {
            "binary files differ"
        }
    );
    Ok(if equal {
        Outcome::Identical
    } else {
        Outcome::Different
    })
}

fn run_folder(args: &Args) -> Result<Outcome, String> {
    let defaults = FolderOptions::default();
    let opts = FolderOptions {
        mode: if args.content {
            CompareMode::Content
        } else {
            CompareMode::Quick
        },
        filters: Filters {
            include: args.include.clone(),
            exclude: args.exclude.clone(),
        },
        case_insensitive: if args.case_sensitive {
            false
        } else {
            defaults.case_insensitive
        },
        quick: QuickRule {
            tolerance: std::time::Duration::from_secs(args.time_tolerance),
            ignore_dst: args.ignore_dst,
        },
        ..defaults
    };
    let progress = FolderProgress::default();
    let cmp = compare_folders(
        &args.left,
        &args.right,
        &opts,
        &progress,
        &CancelToken::new(),
    )
    .map_err(|e| e.to_string())?;
    let changed = cmp
        .nodes
        .iter()
        .filter(|n| n.status != Status::Same)
        .count();
    if args.quiet {
        println!("{}", report::folder_summary(cmp.is_identical(), changed));
    } else if args.json {
        println!("{}", report::folder_json(&cmp));
    } else {
        print!("{}", report::folder_plain(&cmp));
    }
    Ok(if cmp.is_identical() {
        Outcome::Identical
    } else {
        Outcome::Different
    })
}
