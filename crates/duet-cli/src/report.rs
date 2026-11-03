//! Plain-text and JSON renderings of comparison results.

use std::fmt::Write as _;

use duet_core::folder::tree::{FolderComparison, Side, Status};
use duet_core::text::diff::LineDiff;
use duet_core::text::document::TextDocument;
use serde_json::json;

pub const BYTES_DIFFER: &str = "text identical, bytes differ (encoding, BOM or undecodable bytes)";

pub fn summary(identical: bool, hunks: usize, bytes_differ: bool) -> String {
    if identical {
        "identical".into()
    } else if bytes_differ {
        format!("different: {BYTES_DIFFER}")
    } else {
        format!("different: {hunks} hunk(s)")
    }
}

pub fn folder_summary(identical: bool, changed: usize) -> String {
    if identical {
        "identical".into()
    } else {
        format!("different: {changed} row(s)")
    }
}

pub fn text_plain(left: &TextDocument, right: &TextDocument, diff: &LineDiff) -> String {
    let mut out = String::new();
    for hunk in &diff.hunks {
        // Lines that match except for their terminator would print identically; show the
        // terminators so the reader can see why they differ.
        let eol_only = hunk.left.len() == hunk.right.len()
            && hunk
                .left
                .clone()
                .zip(hunk.right.clone())
                .all(|(l, r)| left.line(l as usize) == right.line(r as usize));
        let _ = writeln!(
            out,
            "@@ -{},{} +{},{} @@{}",
            hunk.left.start + 1,
            hunk.left.end - hunk.left.start,
            hunk.right.start + 1,
            hunk.right.end - hunk.right.start,
            if eol_only { " line endings only" } else { "" }
        );
        for line in hunk.left.clone() {
            let _ = writeln!(out, "-{}", shown(left, line as usize, eol_only));
        }
        for line in hunk.right.clone() {
            let _ = writeln!(out, "+{}", shown(right, line as usize, eol_only));
        }
    }
    out
}

fn shown(doc: &TextDocument, line: usize, with_eol: bool) -> String {
    let text = doc.line(line);
    if !with_eol {
        return text.into_owned();
    }
    let eol = match doc.lines().eol_bytes(doc.bytes(), line) {
        b"\r\n" => r"\r\n",
        b"\n" => r"\n",
        b"\r" => r"\r",
        _ => "(no newline)",
    };
    format!("{text}{eol}")
}

pub fn text_json(
    left: &TextDocument,
    right: &TextDocument,
    diff: &LineDiff,
    bytes_differ: bool,
) -> String {
    let hunks: Vec<_> = diff
        .hunks
        .iter()
        .map(|h| json!({ "left": [h.left.start, h.left.end], "right": [h.right.start, h.right.end] }))
        .collect();
    json!({
        "kind": "text",
        "identical": diff.is_identical() && !bytes_differ,
        "bytes_differ": bytes_differ,
        "left_encoding": left.encoding().encoding.name(),
        "right_encoding": right.encoding().encoding.name(),
        "left_bom": left.encoding().bom,
        "right_bom": right.encoding().bom,
        "left_lossy": left.is_lossy(),
        "right_lossy": right.is_lossy(),
        "hunks": hunks,
    })
    .to_string()
}

fn code(status: &Status) -> char {
    match status {
        Status::Same => '=',
        Status::Different { .. } => '!',
        Status::Similar { .. } => '≈',
        Status::LeftOnly => '<',
        Status::RightOnly => '>',
        Status::Pending => '~',
        Status::Error(_) => '?',
    }
}

fn name(status: &Status) -> &'static str {
    match status {
        Status::Same => "same",
        Status::Different { .. } => "different",
        Status::Similar { .. } => "similar",
        Status::LeftOnly => "left_only",
        Status::RightOnly => "right_only",
        Status::Pending => "pending",
        Status::Error(_) => "error",
    }
}

pub fn folder_plain(cmp: &FolderComparison) -> String {
    let mut out = String::new();
    for node in cmp.nodes.iter().filter(|n| n.status != Status::Same) {
        let _ = writeln!(out, "{}\t{}", code(&node.status), node.rel);
    }
    out
}

pub fn folder_json(cmp: &FolderComparison) -> String {
    let nodes: Vec<_> = cmp
        .nodes
        .iter()
        .filter(|n| n.status != Status::Same)
        .map(|n| {
            let newer = match n.status {
                Status::Different { newer } | Status::Similar { newer } => match newer {
                    Some(Side::Left) => Some("left"),
                    Some(Side::Right) => Some("right"),
                    None => None,
                },
                _ => None,
            };
            let error = match &n.status {
                Status::Error(message) => Some(message.as_str()),
                _ => None,
            };
            json!({ "rel": n.rel, "status": name(&n.status), "newer": newer, "error": error })
        })
        .collect();
    json!({ "kind": "folder", "identical": cmp.is_identical(), "nodes": nodes }).to_string()
}
