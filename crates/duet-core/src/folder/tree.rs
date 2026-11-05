//! Pairs two scans into one aligned tree, applies the quick (size + time) rule and rolls
//! child differences up into their folders.

use std::collections::{BTreeMap, HashMap};
use std::time::Duration;

use crate::folder::classify::{Note, Warning};
use crate::folder::entry::{Entry, EntryKind};

pub const MTIME_TOLERANCE: Duration = Duration::from_secs(2);
const DST_SHIFT: Duration = Duration::from_secs(3_600);

/// The quick (size + time) rule. Times within `tolerance` match; with `ignore_dst`, times
/// exactly one hour apart (within the same tolerance) also match.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuickRule {
    pub tolerance: Duration,
    pub ignore_dst: bool,
}

impl Default for QuickRule {
    fn default() -> Self {
        Self {
            tolerance: MTIME_TOLERANCE,
            ignore_dst: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Same,
    Different {
        newer: Option<Side>,
    },
    /// Both files decode to the same text; only line endings, a BOM or the encoding differ.
    /// Content mode only, for files up to `FolderOptions::similar_limit`.
    Similar {
        newer: Option<Side>,
    },
    LeftOnly,
    RightOnly,
    Pending,
    Error(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub name: String,
    pub rel: String,
    pub depth: u32,
    pub parent: Option<u32>,
    pub is_dir: bool,
    pub left: Option<Entry>,
    pub right: Option<Entry>,
    pub status: Status,
    /// What the content compare learned about a file pair (formats, warning).
    pub note: Option<Box<Note>>,
}

impl Node {
    /// Both sides present and their names differ (pairing is case-insensitive, so only in
    /// case). Files only; LocalDiff compares the whole path, this compares the relative one.
    pub fn case_differs(&self) -> bool {
        let (Some(l), Some(r)) = (&self.left, &self.right) else {
            return false;
        };
        !self.is_dir && l.rel != r.rel
    }

    pub fn warning(&self) -> Option<Warning> {
        self.note.as_ref().and_then(|note| note.warning)
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct FolderComparison {
    pub nodes: Vec<Node>,
    /// The rule the comparison was built with (R7).
    pub quick: QuickRule,
    /// Node index to the index of its moved or renamed twin; both directions are present.
    /// Built by `folder::moves` and empty otherwise.
    pub moved: BTreeMap<u32, u32>,
}

impl FolderComparison {
    pub fn moved_partner(&self, index: u32) -> Option<u32> {
        self.moved.get(&index).copied()
    }

    /// Moved pairs (each pair is two rows).
    pub fn moved_files(&self) -> usize {
        self.moved.len() / 2
    }

    /// A file pair on both sides whose modified times differ beyond `self.quick`.
    pub fn time_differs(&self, index: u32) -> bool {
        let node = &self.nodes[index as usize];
        match (&node.left, &node.right) {
            (Some(l), Some(r)) if !node.is_dir => newer_side(l, r, self.quick).is_some(),
            _ => false,
        }
    }

    pub fn is_identical(&self) -> bool {
        self.nodes.iter().all(|n| n.status == Status::Same)
    }

    /// The same comparison seen from the other side (F9): entries, one-sided states and
    /// "newer" flip; paths and order stay.
    pub fn swap_sides(&mut self) {
        let flip = |side: Option<Side>| {
            side.map(|s| match s {
                Side::Left => Side::Right,
                Side::Right => Side::Left,
            })
        };
        for node in &mut self.nodes {
            std::mem::swap(&mut node.left, &mut node.right);
            node.note = node.note.take().map(|note| Box::new(note.swapped()));
            node.status = match std::mem::replace(&mut node.status, Status::Same) {
                Status::LeftOnly => Status::RightOnly,
                Status::RightOnly => Status::LeftOnly,
                Status::Different { newer } => Status::Different { newer: flip(newer) },
                Status::Similar { newer } => Status::Similar { newer: flip(newer) },
                other => other,
            };
        }
    }
}

type Pairs = BTreeMap<String, (Option<Entry>, Option<Entry>)>;

pub fn build_tree(left: Vec<Entry>, right: Vec<Entry>, case_insensitive: bool) -> FolderComparison {
    build_tree_with(left, right, case_insensitive, QuickRule::default())
}

pub fn build_tree_with(
    left: Vec<Entry>,
    right: Vec<Entry>,
    case_insensitive: bool,
    rule: QuickRule,
) -> FolderComparison {
    let key_of = |rel: &str| {
        if case_insensitive {
            rel.to_lowercase()
        } else {
            rel.to_owned()
        }
    };
    let mut pairs = Pairs::new();
    for entry in left {
        let key = key_of(&entry.rel);
        pairs.entry(key).or_default().0 = Some(entry);
    }
    for entry in right {
        let key = key_of(&entry.rel);
        pairs.entry(key).or_default().1 = Some(entry);
    }
    let mut children: HashMap<String, Vec<String>> = HashMap::new();
    for key in pairs.keys() {
        let parent = match key.rsplit_once('/') {
            Some((parent, _)) if pairs.contains_key(parent) => parent.to_owned(),
            _ => String::new(),
        };
        children.entry(parent).or_default().push(key.clone());
    }
    // Pre-order with an explicit stack: deep trees (thousands of levels) must not overflow.
    let mut cmp = FolderComparison {
        quick: rule,
        ..FolderComparison::default()
    };
    let mut stack = Vec::new();
    push_sorted_children(&mut stack, &pairs, &mut children, "", None, 0);
    while let Some((key, parent, depth)) = stack.pop() {
        let Some((left, right)) = pairs.remove(&key) else {
            continue;
        };
        let rel = display_rel(&left, &right).to_owned();
        let name = rel.rsplit('/').next().unwrap_or(&rel).to_owned();
        let status = pair_status_with(left.as_ref(), right.as_ref(), rule);
        let index = cmp.nodes.len() as u32;
        cmp.nodes.push(Node {
            name,
            rel,
            depth,
            parent,
            is_dir: is_dir(&left, &right),
            left,
            right,
            status,
            note: None,
        });
        push_sorted_children(
            &mut stack,
            &pairs,
            &mut children,
            &key,
            Some(index),
            depth + 1,
        );
    }
    rollup(&mut cmp);
    cmp
}

/// Pushes `parent_key`'s children so that they pop in display order.
fn push_sorted_children(
    stack: &mut Vec<(String, Option<u32>, u32)>,
    pairs: &Pairs,
    children: &mut HashMap<String, Vec<String>>,
    parent_key: &str,
    parent: Option<u32>,
    depth: u32,
) {
    let Some(mut keys) = children.remove(parent_key) else {
        return;
    };
    keys.sort_by_cached_key(|key| {
        let (left, right) = &pairs[key];
        let name = display_rel(left, right)
            .rsplit('/')
            .next()
            .unwrap_or_default()
            .to_owned();
        (!is_dir(left, right), name.to_lowercase(), name)
    });
    stack.extend(keys.into_iter().rev().map(|key| (key, parent, depth)));
}

fn display_rel<'a>(left: &'a Option<Entry>, right: &'a Option<Entry>) -> &'a str {
    left.as_ref()
        .or(right.as_ref())
        .map_or("", |e| e.rel.as_str())
}

fn is_dir(left: &Option<Entry>, right: &Option<Entry>) -> bool {
    [left, right]
        .iter()
        .any(|e| e.as_ref().is_some_and(|e| e.kind == EntryKind::Dir))
}

pub fn pair_status(left: Option<&Entry>, right: Option<&Entry>) -> Status {
    pair_status_with(left, right, QuickRule::default())
}

pub fn pair_status_with(left: Option<&Entry>, right: Option<&Entry>, rule: QuickRule) -> Status {
    match (left, right) {
        (Some(_), None) => Status::LeftOnly,
        (None, Some(_)) => Status::RightOnly,
        (None, None) => Status::Same,
        (Some(l), Some(r)) => {
            if let Some(message) = l.error.as_ref().or(r.error.as_ref()) {
                return Status::Error(message.clone());
            }
            match (l.kind, r.kind) {
                (EntryKind::Dir, EntryKind::Dir) => Status::Same,
                (a, b) if a != b => Status::Different { newer: None },
                // A link's own size and time say nothing about what it points at.
                (EntryKind::Symlink, EntryKind::Symlink) => {
                    match (&l.link_target, &r.link_target) {
                        (Some(a), Some(b)) if a == b => Status::Same,
                        (Some(_), Some(_)) => Status::Different { newer: None },
                        _ => {
                            Status::Error(crate::i18n::tr_key("cannot read the link target").into())
                        }
                    }
                }
                _ => quick_status_with(l, r, rule),
            }
        }
    }
}

pub fn quick_status(left: &Entry, right: &Entry) -> Status {
    quick_status_with(left, right, QuickRule::default())
}

pub fn quick_status_with(left: &Entry, right: &Entry, rule: QuickRule) -> Status {
    let (newer, times_match) = times(left, right, rule);
    if left.len == right.len && times_match {
        Status::Same
    } else {
        Status::Different { newer }
    }
}

/// The side that is newer beyond the rule (`None` when the times match or one is unknown).
pub fn newer_side(left: &Entry, right: &Entry, rule: QuickRule) -> Option<Side> {
    times(left, right, rule).0
}

/// (the newer side beyond the rule, whether the times match).
fn times(left: &Entry, right: &Entry, rule: QuickRule) -> (Option<Side>, bool) {
    match (left.modified, right.modified) {
        (Some(a), Some(b)) => {
            let (gap, side) = if a >= b {
                (a.duration_since(b).unwrap_or_default(), Side::Left)
            } else {
                (b.duration_since(a).unwrap_or_default(), Side::Right)
            };
            let within = |target: Duration| gap.abs_diff(target) <= rule.tolerance;
            if within(Duration::ZERO) || (rule.ignore_dst && within(DST_SHIFT)) {
                (None, true)
            } else {
                (Some(side), false)
            }
        }
        _ => (None, false),
    }
}

/// Size only: a file pair is Same exactly when the lengths match, else Different with the
/// side newer by `cmp.quick`. Folders, links, one-sided rows and errors keep their status; the
/// caller rolls up.
pub fn apply_size_rule(cmp: &mut FolderComparison) {
    let quick = cmp.quick;
    for node in &mut cmp.nodes {
        let (Some(l), Some(r)) = (&node.left, &node.right) else {
            continue;
        };
        let files = l.kind == EntryKind::File
            && r.kind == EntryKind::File
            && l.error.is_none()
            && r.error.is_none();
        if files {
            node.status = if l.len == r.len {
                Status::Same
            } else {
                Status::Different {
                    newer: newer_side(l, r, quick),
                }
            };
        }
    }
}

pub fn rollup(cmp: &mut FolderComparison) {
    let count = cmp.nodes.len();
    let mut has_diff = vec![false; count];
    let mut has_pending = vec![false; count];
    let mut has_similar = vec![false; count];
    // Pre-order puts every descendant after its folder, so a reverse walk sees children first.
    for i in (0..count).rev() {
        let node = &mut cmp.nodes[i];
        let both_dirs = [&node.left, &node.right].iter().all(|e| {
            e.as_ref()
                .is_some_and(|e| e.kind == EntryKind::Dir && e.error.is_none())
        });
        if both_dirs {
            node.status = if has_diff[i] {
                Status::Different { newer: None }
            } else if has_pending[i] {
                Status::Pending
            } else if has_similar[i] {
                Status::Similar { newer: None }
            } else {
                Status::Same
            };
        }
        if let Some(parent) = node.parent {
            match node.status {
                Status::Same => {}
                Status::Pending => has_pending[parent as usize] = true,
                Status::Similar { .. } => has_similar[parent as usize] = true,
                _ => has_diff[parent as usize] = true,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn at(secs: u64) -> Option<SystemTime> {
        Some(UNIX_EPOCH + Duration::from_secs(secs))
    }

    fn file(rel: &str, len: u64, secs: u64) -> Entry {
        Entry {
            rel: rel.into(),
            kind: EntryKind::File,
            len,
            modified: at(secs),
            error: None,
            link_target: None,
        }
    }

    fn dir(rel: &str) -> Entry {
        Entry {
            rel: rel.into(),
            kind: EntryKind::Dir,
            len: 0,
            modified: at(0),
            error: None,
            link_target: None,
        }
    }

    fn summary(cmp: &FolderComparison) -> Vec<(&str, Status)> {
        cmp.nodes
            .iter()
            .map(|n| (n.rel.as_str(), n.status.clone()))
            .collect()
    }

    #[test]
    fn identical_file_is_same() {
        let cmp = build_tree(
            vec![file("a.txt", 3, 100)],
            vec![file("a.txt", 3, 100)],
            true,
        );
        assert_eq!(summary(&cmp), vec![("a.txt", Status::Same)]);
        assert!(cmp.is_identical());
    }

    #[test]
    fn quick_rule_uses_size_and_two_second_tolerance() {
        let different_size = build_tree(vec![file("a", 3, 100)], vec![file("a", 4, 100)], true);
        assert_eq!(
            different_size.nodes[0].status,
            Status::Different { newer: None }
        );
        let within = build_tree(vec![file("a", 3, 100)], vec![file("a", 3, 102)], true);
        assert_eq!(within.nodes[0].status, Status::Same);
        let newer_left = build_tree(vec![file("a", 3, 105)], vec![file("a", 3, 100)], true);
        assert_eq!(
            newer_left.nodes[0].status,
            Status::Different {
                newer: Some(Side::Left)
            }
        );
    }

    #[test]
    fn case_insensitive_pairing() {
        let cmp = build_tree(
            vec![file("Readme.md", 1, 1)],
            vec![file("README.md", 1, 1)],
            true,
        );
        assert_eq!(summary(&cmp), vec![("Readme.md", Status::Same)]);
        let strict = build_tree(
            vec![file("Readme.md", 1, 1)],
            vec![file("README.md", 1, 1)],
            false,
        );
        assert_eq!(
            summary(&strict),
            vec![
                ("README.md", Status::RightOnly),
                ("Readme.md", Status::LeftOnly)
            ]
        );
    }

    #[test]
    fn orphans_and_rollup_with_tree_shape() {
        let left = vec![dir("a"), dir("a/b"), file("a/b/c.txt", 1, 1)];
        let right = vec![
            dir("a"),
            dir("a/b"),
            file("a/b/c.txt", 2, 1),
            file("z.txt", 1, 1),
        ];
        let cmp = build_tree(left, right, true);
        assert_eq!(
            summary(&cmp),
            vec![
                ("a", Status::Different { newer: None }),
                ("a/b", Status::Different { newer: None }),
                ("a/b/c.txt", Status::Different { newer: None }),
                ("z.txt", Status::RightOnly),
            ]
        );
        let depths: Vec<_> = cmp.nodes.iter().map(|n| (n.depth, n.parent)).collect();
        assert_eq!(
            depths,
            vec![(0, None), (1, Some(0)), (2, Some(1)), (0, None)]
        );
        assert!(!cmp.is_identical());
    }

    #[test]
    fn folders_sort_before_files() {
        let cmp = build_tree(
            vec![file("a.txt", 1, 1), dir("b")],
            vec![file("a.txt", 1, 1), dir("b")],
            true,
        );
        assert_eq!(
            cmp.nodes.iter().map(|n| n.rel.as_str()).collect::<Vec<_>>(),
            vec!["b", "a.txt"]
        );
    }

    #[test]
    fn file_versus_folder_is_different() {
        let cmp = build_tree(vec![file("x", 1, 1)], vec![dir("x")], true);
        assert_eq!(cmp.nodes[0].status, Status::Different { newer: None });
        assert!(cmp.nodes[0].is_dir);
    }

    #[test]
    fn errors_become_rows_and_mark_parents() {
        let mut broken = file("d/f.txt", 0, 1);
        broken.error = Some("access denied".into());
        let cmp = build_tree(
            vec![dir("d"), broken],
            vec![dir("d"), file("d/f.txt", 0, 1)],
            true,
        );
        assert_eq!(cmp.nodes[1].status, Status::Error("access denied".into()));
        assert_eq!(cmp.nodes[0].status, Status::Different { newer: None });
    }

    #[test]
    fn pending_children_make_folder_pending() {
        let mut cmp = build_tree(
            vec![dir("d"), file("d/f", 1, 1)],
            vec![dir("d"), file("d/f", 1, 1)],
            true,
        );
        cmp.nodes[1].status = Status::Pending;
        rollup(&mut cmp);
        assert_eq!(cmp.nodes[0].status, Status::Pending);
    }

    #[test]
    fn thai_names_pair() {
        let cmp = build_tree(
            vec![file("เอกสาร/งบ.xlsx", 9, 9)],
            vec![file("เอกสาร/งบ.xlsx", 9, 9)],
            true,
        );
        assert_eq!(cmp.nodes[0].status, Status::Same);
        assert_eq!(cmp.nodes[0].name, "งบ.xlsx");
    }

    #[test]
    fn very_deep_trees_build_without_recursion() {
        // 2,000 levels in a 256 KiB stack: a frame per level overflowed at ~1,500 levels in 1 MiB.
        let chain = |levels: usize| -> Vec<Entry> {
            let mut rel = String::new();
            let mut out = Vec::with_capacity(levels);
            for _ in 0..levels {
                if !rel.is_empty() {
                    rel.push('/');
                }
                rel.push('a');
                out.push(dir(&rel));
            }
            out
        };
        let built = std::thread::Builder::new()
            .stack_size(256 * 1024)
            .spawn(move || build_tree(chain(2_000), chain(2_000), true))
            .unwrap()
            .join()
            .unwrap();
        assert_eq!(built.nodes.len(), 2_000);
        assert_eq!(built.nodes[1_999].depth, 1_999);
        assert_eq!(built.nodes[1_999].parent, Some(1_998));
        assert!(built.is_identical());
    }

    #[test]
    fn link_pairs_compare_by_target_and_ignore_link_times() {
        let link = |rel: &str, target: &str, secs: u64| Entry {
            kind: EntryKind::Symlink,
            link_target: Some(target.into()),
            ..file(rel, 0, secs)
        };
        let cmp = build_tree(
            vec![
                link("a", "../t1", 10),
                link("b", "../t1", 10),
                file("c", 0, 10),
            ],
            vec![
                link("a", "../t1", 9_000),
                link("b", "../t2", 10),
                Entry {
                    kind: EntryKind::Symlink,
                    ..file("c", 0, 10)
                },
            ],
            true,
        );
        assert_eq!(
            summary(&cmp),
            vec![
                ("a", Status::Same),
                ("b", Status::Different { newer: None }),
                ("c", Status::Different { newer: None }),
            ]
        );
    }

    #[test]
    fn quick_rule_tolerance_and_dst_shift() {
        let hour = 3_600;
        let shifted = |rule| {
            build_tree_with(
                vec![file("a", 3, 100 + hour)],
                vec![file("a", 3, 100)],
                true,
                rule,
            )
            .nodes[0]
                .status
                .clone()
        };
        assert_eq!(
            shifted(QuickRule::default()),
            Status::Different {
                newer: Some(Side::Left)
            }
        );
        let dst = QuickRule {
            ignore_dst: true,
            ..QuickRule::default()
        };
        assert_eq!(shifted(dst), Status::Same);
        let off_by_hour_and_two = build_tree_with(
            vec![file("a", 3, 102 + hour)],
            vec![file("a", 3, 100)],
            true,
            dst,
        );
        assert_eq!(off_by_hour_and_two.nodes[0].status, Status::Same);
        let loose = QuickRule {
            tolerance: Duration::from_secs(10),
            ignore_dst: false,
        };
        let five_apart = build_tree_with(
            vec![file("a", 3, 105)],
            vec![file("a", 3, 100)],
            true,
            loose,
        );
        assert_eq!(five_apart.nodes[0].status, Status::Same);
        let size_wins = build_tree_with(
            vec![file("a", 3, 100 + hour)],
            vec![file("a", 4, 100)],
            true,
            dst,
        );
        assert!(matches!(
            size_wins.nodes[0].status,
            Status::Different { .. }
        ));
    }

    #[test]
    fn a_folder_with_only_text_equal_files_rolls_up_as_similar() {
        let mut cmp = build_tree(
            vec![dir("d"), file("d/a", 1, 0)],
            vec![dir("d"), file("d/a", 1, 0)],
            true,
        );
        cmp.nodes[1].status = Status::Similar { newer: None };
        rollup(&mut cmp);
        assert_eq!(cmp.nodes[0].status, Status::Similar { newer: None });
        cmp.nodes.push(Node {
            status: Status::Different { newer: None },
            parent: Some(0),
            ..cmp.nodes[1].clone()
        });
        rollup(&mut cmp);
        assert_eq!(
            cmp.nodes[0].status,
            Status::Different { newer: None },
            "a real difference wins"
        );
    }

    #[test]
    fn swapping_a_comparison_flips_one_sided_states_and_newer() {
        let mut cmp = build_tree(
            vec![file("l.txt", 1, 0), file("x.txt", 1, 100)],
            vec![file("r.txt", 1, 0), file("x.txt", 1, 0)],
            true,
        );
        let before = summary(&cmp)
            .into_iter()
            .map(|(rel, status)| (rel.to_owned(), status))
            .collect::<Vec<_>>();
        cmp.swap_sides();
        let status = |rel: &str| {
            cmp.nodes
                .iter()
                .find(|n| n.rel == rel)
                .unwrap()
                .status
                .clone()
        };
        assert_eq!(status("l.txt"), Status::RightOnly);
        assert_eq!(status("r.txt"), Status::LeftOnly);
        assert_eq!(
            status("x.txt"),
            Status::Different {
                newer: Some(Side::Right)
            }
        );
        assert!(
            cmp.nodes
                .iter()
                .find(|n| n.rel == "l.txt")
                .unwrap()
                .right
                .is_some()
        );
        cmp.swap_sides();
        let after = summary(&cmp)
            .into_iter()
            .map(|(rel, status)| (rel.to_owned(), status))
            .collect::<Vec<_>>();
        assert_eq!(after, before, "swapping twice is the identity");
    }
}
