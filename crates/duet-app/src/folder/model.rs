//! Folder-view state without any GUI: which rows are visible (expansion + filter), the
//! selection, difference navigation and per-status counts.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use duet_core::folder::classify::Warning;
use duet_core::folder::entry::EntryKind;
use duet_core::folder::tree::{FolderComparison, Node, Status};

use crate::folder::prefs::Layout;
use crate::status::Tone;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FolderFilter {
    All,
    #[default]
    Mismatch,
    Different,
    LeftOnly,
    RightOnly,
    Same,
}

const SAME: u8 = 1;
const DIFF: u8 = 2;
const LEFT: u8 = 4;
const RIGHT: u8 = 8;
const ERROR: u8 = 16;
const PENDING: u8 = 32;
const SIMILAR: u8 = 64;
/// `row_of` for a node that is not a row.
const NO_ROW: u32 = u32::MAX;

fn flag(status: &Status) -> u8 {
    match status {
        Status::Same => SAME,
        Status::Different { .. } => DIFF,
        Status::Similar { .. } => SIMILAR,
        Status::LeftOnly => LEFT,
        Status::RightOnly => RIGHT,
        Status::Error(_) => ERROR,
        Status::Pending => PENDING,
    }
}

impl FolderFilter {
    pub const ALL: [FolderFilter; 6] = [
        FolderFilter::All,
        FolderFilter::Mismatch,
        FolderFilter::Different,
        FolderFilter::LeftOnly,
        FolderFilter::RightOnly,
        FolderFilter::Same,
    ];

    /// Statuses a row's subtree must contain to stay visible. Pending rows (still being
    /// compared) show in every filter except Same.
    fn mask(self) -> u8 {
        match self {
            FolderFilter::All => u8::MAX,
            FolderFilter::Mismatch => DIFF | LEFT | RIGHT | ERROR | PENDING,
            FolderFilter::Different => DIFF | ERROR | PENDING,
            FolderFilter::LeftOnly => LEFT | PENDING,
            FolderFilter::RightOnly => RIGHT | PENDING,
            FolderFilter::Same => SAME | SIMILAR,
        }
    }

    /// The key `settings.json` keeps the filter under.
    pub fn key(self) -> &'static str {
        match self {
            FolderFilter::All => "all",
            FolderFilter::Mismatch => "mismatch",
            FolderFilter::Different => "different",
            FolderFilter::LeftOnly => "left",
            FolderFilter::RightOnly => "right",
            FolderFilter::Same => "same",
        }
    }

    pub fn from_key(key: &str) -> Option<FolderFilter> {
        FolderFilter::ALL.into_iter().find(|f| f.key() == key)
    }

    /// The filter that shows the same rows once the sides are swapped.
    pub fn mirrored(self) -> Self {
        match self {
            FolderFilter::LeftOnly => FolderFilter::RightOnly,
            FolderFilter::RightOnly => FolderFilter::LeftOnly,
            other => other,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            FolderFilter::All => "All",
            FolderFilter::Mismatch => "Mismatches",
            FolderFilter::Different => "Different",
            FolderFilter::LeftOnly => "Left only",
            FolderFilter::RightOnly => "Right only",
            FolderFilter::Same => "Same",
        }
    }

    /// LocalDiff's chip words; `label` keeps the status line's.
    pub fn chip_label(self) -> &'static str {
        match self {
            FolderFilter::All => "All",
            FolderFilter::Mismatch => "Mismatched",
            FolderFilter::Different => "Changed",
            FolderFilter::LeftOnly => "Left only",
            FolderFilter::RightOnly => "Right only",
            FolderFilter::Same => "Same",
        }
    }

    pub fn chip_id(self) -> &'static str {
        match self {
            FolderFilter::All => "folder-chip-all",
            FolderFilter::Mismatch => "folder-chip-mismatch",
            FolderFilter::Different => "folder-chip-changed",
            FolderFilter::LeftOnly => "folder-chip-left-only",
            FolderFilter::RightOnly => "folder-chip-right-only",
            FolderFilter::Same => "folder-chip-same",
        }
    }

    pub fn chip_tone(self) -> Tone {
        match self {
            FolderFilter::All | FolderFilter::Same => Tone::Text,
            FolderFilter::Mismatch => Tone::Accent,
            FolderFilter::Different => Tone::Changed,
            FolderFilter::LeftOnly => Tone::LeftOnly,
            FolderFilter::RightOnly => Tone::RightOnly,
        }
    }

    /// The rows the chip's filter keeps, as the chip shows it.
    pub fn count(self, counts: &Counts) -> usize {
        match self {
            FolderFilter::All => counts.all,
            FolderFilter::Mismatch => {
                counts.different + counts.left_only + counts.right_only + counts.errors
            }
            FolderFilter::Different => counts.different + counts.errors,
            FolderFilter::LeftOnly => counts.left_only,
            FolderFilter::RightOnly => counts.right_only,
            FolderFilter::Same => counts.same + counts.similar,
        }
    }
}

/// The column the flat list is sorted by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortKey {
    #[default]
    Path,
    Status,
    LeftSize,
    RightSize,
    LeftTime,
    RightTime,
}

/// The leaf files below a node, by what the compare says about them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Breakdown {
    pub different: u32,
    pub left_only: u32,
    pub right_only: u32,
    pub same: u32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counts {
    pub all: usize,
    pub different: usize,
    pub left_only: usize,
    pub right_only: usize,
    pub same: usize,
    pub errors: usize,
    pub pending: usize,
    /// Files whose text is the same, with only EOL, BOM or encoding differences ("≈"); they
    /// count as same. `soft` and `format_only` split them by why.
    pub similar: usize,
    /// Leaf files present on each side, and their total size in bytes.
    pub left_files: usize,
    pub right_files: usize,
    pub left_bytes: u64,
    pub right_bytes: u64,
    /// Same text, the rules hid a difference ("≈", unimportant).
    pub soft: usize,
    /// Same text, only line endings, final newline or BOM differ ("≈").
    pub format_only: usize,
    /// Same or "≈" files whose modified times differ beyond the rule.
    pub time_only: usize,
    /// Scripts with CRLF or a BOM ("⚠").
    pub script: usize,
    /// Only the kind of whitespace differs ("⚠").
    pub odd_space: usize,
    /// File pairs whose names differ in case.
    pub case_names: usize,
}

#[derive(Clone)]
pub struct FolderModel {
    cmp: Arc<FolderComparison>,
    leaf: Vec<bool>,
    /// Status flags of the node (if a leaf) and all its descendants.
    subtree: Vec<u8>,
    /// Leaf rows at or below each node that are neither Same, "≈" nor Pending.
    differing: Vec<u32>,
    /// Per node: the leaf files below by verdict, summed once in `new` (folders are not files).
    breakdowns: Vec<Breakdown>,
    /// Per node and side (left, right): the bytes of the files below, and the newest modified
    /// time among them.
    bytes: Vec<[u64; 2]>,
    newest: Vec<[Option<SystemTime>; 2]>,
    /// Per node: the leaf files below that the chips and the path filter keep. Rebuilt with
    /// `visible`, in one reverse pass.
    shown_below: Vec<u32>,
    expanded: Vec<bool>,
    filter: FolderFilter,
    /// Node indices in display order: pre-order (ascending) in the tree, the sorted files in
    /// the flat list.
    visible: Vec<u32>,
    /// Per node: its row in `visible`, or `NO_ROW`. Rebuilt with `visible`.
    row_of: Vec<u32>,
    layout: Layout,
    sort_key: SortKey,
    sort_desc: bool,
    /// The cursor: the row keys and the detail pane follow.
    selected: Option<u32>,
    /// Every chosen node (Ctrl/Shift/drag), all visible; empty means just the cursor.
    chosen: BTreeSet<u32>,
    /// Where a Shift range starts.
    anchor: Option<u32>,
    /// The lowercased name filter, and which nodes match it or hold a match.
    name_filter: Option<String>,
    name_hit: Vec<bool>,
    counts: Counts,
}

impl FolderModel {
    pub fn new(cmp: FolderComparison) -> Self {
        let n = cmp.nodes.len();
        let mut leaf = vec![true; n];
        for node in &cmp.nodes {
            if let Some(parent) = node.parent {
                leaf[parent as usize] = false;
            }
        }
        let mut subtree = vec![0u8; n];
        // Pre-order: every child comes after its parent, so a reverse pass sees children first.
        for i in (0..n).rev() {
            if leaf[i] {
                subtree[i] |= flag(&cmp.nodes[i].status);
            }
            if let Some(parent) = cmp.nodes[i].parent {
                let flags = subtree[i];
                subtree[parent as usize] |= flags;
            }
        }
        let mut differing = vec![0u32; n];
        for i in (0..n).rev() {
            if leaf[i]
                && !matches!(
                    cmp.nodes[i].status,
                    Status::Same | Status::Similar { .. } | Status::Pending
                )
            {
                differing[i] += 1;
            }
            if let Some(parent) = cmp.nodes[i].parent {
                let below = differing[i];
                differing[parent as usize] += below;
            }
        }
        let mut breakdowns = vec![Breakdown::default(); n];
        let mut bytes = vec![[0u64; 2]; n];
        let mut newest: Vec<[Option<SystemTime>; 2]> = vec![[None; 2]; n];
        for i in (0..n).rev() {
            let node = &cmp.nodes[i];
            if leaf[i] && !node.is_dir {
                let b = &mut breakdowns[i];
                match node.status {
                    Status::Different { .. } | Status::Error(_) => b.different += 1,
                    Status::LeftOnly => b.left_only += 1,
                    Status::RightOnly => b.right_only += 1,
                    Status::Same | Status::Similar { .. } => b.same += 1,
                    Status::Pending => {}
                }
                for (side, entry) in [&node.left, &node.right].into_iter().enumerate() {
                    if let Some(e) = entry.as_ref().filter(|e| e.kind == EntryKind::File) {
                        bytes[i][side] = e.len;
                        newest[i][side] = e.modified;
                    }
                }
            }
            if let Some(parent) = node.parent {
                let p = parent as usize;
                let (b, by, nw) = (breakdowns[i], bytes[i], newest[i]);
                breakdowns[p].different += b.different;
                breakdowns[p].left_only += b.left_only;
                breakdowns[p].right_only += b.right_only;
                breakdowns[p].same += b.same;
                for side in 0..2 {
                    bytes[p][side] += by[side];
                    newest[p][side] = newest[p][side].max(nw[side]);
                }
            }
        }
        // Folders on both sides that hold a mismatch start open, so differences show at once.
        let expanded = cmp
            .nodes
            .iter()
            .zip(&subtree)
            .map(|(node, flags)| {
                node.is_dir
                    && node.left.is_some()
                    && node.right.is_some()
                    && flags & FolderFilter::Mismatch.mask() != 0
            })
            .collect();
        let counts = Self::count(&cmp, &leaf);
        let mut model = Self {
            counts,
            cmp: Arc::new(cmp),
            leaf,
            subtree,
            differing,
            breakdowns,
            bytes,
            newest,
            shown_below: Vec::new(),
            expanded,
            filter: FolderFilter::default(),
            visible: Vec::new(),
            row_of: Vec::new(),
            layout: Layout::default(),
            sort_key: SortKey::default(),
            sort_desc: false,
            selected: None,
            chosen: BTreeSet::new(),
            anchor: None,
            name_filter: None,
            name_hit: Vec::new(),
        };
        model.rebuild();
        model
    }

    fn rebuild(&mut self) {
        let mask = self.filter.mask();
        self.visible.clear();
        let filtering = self.name_filter.is_some();
        if self.layout == Layout::Flat {
            // Files only, in the sorted order; sorted here, once per change, never per frame.
            for (i, node) in self.cmp.nodes.iter().enumerate() {
                if !node.is_dir && self.subtree[i] & mask != 0 && (!filtering || self.name_hit[i]) {
                    self.visible.push(i as u32);
                }
            }
            self.sort_visible();
        } else {
            let mut shown = vec![false; self.cmp.nodes.len()];
            for (i, node) in self.cmp.nodes.iter().enumerate() {
                let parent_open = node
                    .parent
                    .is_none_or(|p| shown[p as usize] && (self.expanded[p as usize] || filtering));
                shown[i] =
                    parent_open && self.subtree[i] & mask != 0 && (!filtering || self.name_hit[i]);
                if shown[i] {
                    self.visible.push(i as u32);
                }
            }
        }
        self.row_of.clear();
        self.row_of.resize(self.cmp.nodes.len(), NO_ROW);
        for (row, &node) in self.visible.iter().enumerate() {
            self.row_of[node as usize] = row as u32;
        }
        // One reverse pass: children come after their parent.
        let nodes = &self.cmp.nodes;
        self.shown_below.clear();
        self.shown_below.resize(nodes.len(), 0);
        for i in (0..nodes.len()).rev() {
            if self.leaf[i]
                && !nodes[i].is_dir
                && self.subtree[i] & mask != 0
                && (!filtering || self.name_hit[i])
            {
                self.shown_below[i] += 1;
            }
            if let Some(parent) = nodes[i].parent {
                self.shown_below[parent as usize] += self.shown_below[i];
            }
        }
        if let Some(node) = self.selected
            && self.row_of_node(node).is_none()
        {
            self.selected = if self.layout == Layout::Flat {
                // The rows are not in node order: a lost selection falls to the first row.
                self.visible.first().copied()
            } else {
                // Keep the selection near where it was: the closest visible row above it.
                let at = self.visible.partition_point(|&v| v < node);
                at.checked_sub(1)
                    .or((!self.visible.is_empty()).then_some(0))
                    .map(|row| self.visible[row])
            };
        }
        let row_of = &self.row_of;
        self.chosen.retain(|&n| row_of[n as usize] != NO_ROW);
        if self.chosen.is_empty()
            && let Some(node) = self.selected
        {
            self.chosen.insert(node);
        }
        if self.anchor.is_none_or(|a| self.row_of_node(a).is_none()) {
            self.anchor = self.selected;
        }
    }

    /// Orders `visible` (the flat list's files) by the sort column. A missing side sorts
    /// lowest; ties go by the lower-cased path, ascending in both directions.
    fn sort_visible(&mut self) {
        let nodes = &self.cmp.nodes;
        let (key, desc) = (self.sort_key, self.sort_desc);
        let lower = |i: u32| nodes[i as usize].rel.to_lowercase();
        if key == SortKey::Path {
            self.visible.sort_by_cached_key(|&i| lower(i));
            if desc {
                self.visible.reverse();
            }
            return;
        }
        let missing = i128::MIN / 2;
        let size = |entry: &Option<duet_core::folder::entry::Entry>| {
            entry.as_ref().map_or(missing, |e| i128::from(e.len))
        };
        let time = |entry: &Option<duet_core::folder::entry::Entry>| {
            entry
                .as_ref()
                .and_then(|e| e.modified)
                .map_or(missing, |t| match t.duration_since(UNIX_EPOCH) {
                    Ok(d) => d.as_nanos() as i128,
                    Err(e) => -(e.duration().as_nanos() as i128),
                })
        };
        let value = |i: u32| -> i128 {
            let node = &nodes[i as usize];
            match key {
                SortKey::Path => 0,
                SortKey::Status => match node.status {
                    Status::Different { .. } | Status::Error(_) => 0,
                    Status::LeftOnly => 1,
                    Status::RightOnly => 2,
                    Status::Same | Status::Similar { .. } | Status::Pending => 3,
                },
                SortKey::LeftSize => size(&node.left),
                SortKey::RightSize => size(&node.right),
                SortKey::LeftTime => time(&node.left),
                SortKey::RightTime => time(&node.right),
            }
        };
        self.visible.sort_by_cached_key(|&i| {
            let v = value(i);
            (if desc { -v } else { v }, lower(i))
        });
    }

    pub fn layout(&self) -> Layout {
        self.layout
    }

    pub fn set_layout(&mut self, layout: Layout) {
        if self.layout == layout {
            return;
        }
        self.layout = layout;
        if layout == Layout::Tree
            && let Some(node) = self.selected
        {
            self.reveal(node as usize);
        }
        self.rebuild();
    }

    /// The flat list's column and whether it runs from high to low.
    pub fn sort(&self) -> (SortKey, bool) {
        (self.sort_key, self.sort_desc)
    }

    /// A header click: the same column flips the order, a new one starts ascending.
    pub fn sort_by(&mut self, key: SortKey) {
        if key == self.sort_key {
            self.sort_desc = !self.sort_desc;
        } else {
            self.sort_key = key;
            self.sort_desc = false;
        }
        if self.layout == Layout::Flat {
            self.rebuild();
        }
    }

    /// The row a node is drawn on, None when the filters or a collapsed folder hide it.
    pub fn row_of_node(&self, node: u32) -> Option<usize> {
        self.row_of
            .get(node as usize)
            .copied()
            .filter(|&row| row != NO_ROW)
            .map(|row| row as usize)
    }

    pub fn comparison(&self) -> &FolderComparison {
        &self.cmp
    }

    /// The same tree shared, for work that outlives the borrow (a delete plan on a worker).
    pub fn shared_comparison(&self) -> Arc<FolderComparison> {
        self.cmp.clone()
    }

    pub fn node(&self, index: u32) -> &Node {
        &self.cmp.nodes[index as usize]
    }

    pub fn has_children(&self, index: u32) -> bool {
        !self.leaf[index as usize]
    }

    pub fn differing_below(&self, index: u32) -> u32 {
        self.differing[index as usize]
    }

    /// The leaf files below `index` that the chips and the path filter keep (folders do not
    /// count), whether or not a folder above is collapsed.
    pub fn files_shown_below(&self, index: u32) -> u32 {
        self.shown_below[index as usize]
    }

    /// Every leaf file below `index`, whatever the filters: different (and errors), left
    /// only, right only, same (and "≈").
    pub fn breakdown(&self, index: u32) -> Breakdown {
        self.breakdowns[index as usize]
    }

    /// The total size of the files below on one side; None when the node is missing there.
    pub fn folder_size(&self, index: u32, left: bool) -> Option<u64> {
        let side = usize::from(!left);
        self.side_entry(index, left)?;
        Some(self.bytes[index as usize][side])
    }

    /// The newest modified time among the files below on one side, else the node's own time;
    /// None when the node is missing there.
    pub fn folder_newest(&self, index: u32, left: bool) -> Option<SystemTime> {
        let entry = self.side_entry(index, left)?;
        self.newest[index as usize][usize::from(!left)].or(entry.modified)
    }

    fn side_entry(&self, index: u32, left: bool) -> Option<&duet_core::folder::entry::Entry> {
        let node = &self.cmp.nodes[index as usize];
        if left {
            node.left.as_ref()
        } else {
            node.right.as_ref()
        }
    }

    pub fn is_expanded(&self, index: u32) -> bool {
        self.expanded[index as usize]
    }

    pub fn filter(&self) -> FolderFilter {
        self.filter
    }

    pub fn set_filter(&mut self, filter: FolderFilter) {
        self.filter = filter;
        self.rebuild();
    }

    pub fn visible_len(&self) -> usize {
        self.visible.len()
    }

    pub fn node_index(&self, row: usize) -> u32 {
        self.visible[row]
    }

    pub fn node_at(&self, row: usize) -> &Node {
        self.node(self.visible[row])
    }

    pub fn selected_row(&self) -> Option<usize> {
        self.selected.and_then(|node| self.row_of_node(node))
    }

    pub fn selected_node(&self) -> Option<&Node> {
        self.selected.map(|node| self.node(node))
    }

    pub fn select_row(&mut self, row: usize) {
        self.set_single(self.visible.get(row).copied());
    }

    /// Cursor, anchor and the whole set become `node`.
    fn set_single(&mut self, node: Option<u32>) {
        self.selected = node;
        self.anchor = node;
        self.chosen.clear();
        self.chosen.extend(node);
    }

    /// Moves the cursor and anchor to `row`, leaving the chosen set as it is.
    pub fn move_cursor(&mut self, row: usize) {
        if let Some(&node) = self.visible.get(row) {
            self.selected = Some(node);
            self.anchor = Some(node);
        }
    }

    /// Ctrl+click: flips the row in the set and makes it the cursor and anchor.
    pub fn toggle_chosen(&mut self, row: usize) {
        let Some(&node) = self.visible.get(row) else {
            return;
        };
        if !self.chosen.remove(&node) {
            self.chosen.insert(node);
        }
        self.selected = Some(node);
        self.anchor = Some(node);
    }

    /// Shift+click: the rows from the anchor to `row`; `keep` (Ctrl held) adds them to the set.
    pub fn extend_to(&mut self, row: usize, keep: bool) {
        let Some(last) = self.visible.len().checked_sub(1) else {
            return;
        };
        let row = row.min(last);
        let node = self.visible[row];
        let anchor_row = self.anchor.and_then(|a| self.row_of_node(a)).unwrap_or(row);
        let (lo, hi) = (anchor_row.min(row), anchor_row.max(row));
        if !keep {
            self.chosen.clear();
        }
        self.chosen.extend(self.visible[lo..=hi].iter().copied());
        self.selected = Some(node);
        self.anchor.get_or_insert(node);
    }

    /// The chosen set as it is now, for a drag to build on.
    pub fn chosen_snapshot(&self) -> BTreeSet<u32> {
        self.chosen.clone()
    }

    /// Drag-select: the set becomes `base` plus the visible rows `anchor_row..=row` (either
    /// way round), and the cursor moves to `row`. The anchor stays, so sweeping back shrinks.
    pub fn set_range_over(&mut self, base: &BTreeSet<u32>, anchor_row: usize, row: usize) {
        let Some(last) = self.visible.len().checked_sub(1) else {
            return;
        };
        let (anchor_row, row) = (anchor_row.min(last), row.min(last));
        let (lo, hi) = (anchor_row.min(row), anchor_row.max(row));
        self.chosen.clone_from(base);
        self.chosen.extend(self.visible[lo..=hi].iter().copied());
        self.selected = Some(self.visible[row]);
    }

    /// Shift+arrows: moves the cursor by `delta` rows, keeping the anchor.
    pub fn extend_by(&mut self, delta: isize) {
        let Some(last) = self.visible.len().checked_sub(1) else {
            return;
        };
        let target = self
            .selected_row()
            .map_or(0, |row| row.saturating_add_signed(delta).min(last));
        self.extend_to(target, false);
    }

    /// Ctrl+A: every visible row. The cursor stays, or becomes the first row.
    pub fn select_all(&mut self) {
        if self.visible.is_empty() {
            return;
        }
        self.chosen = self.visible.iter().copied().collect();
        let cursor = self.selected.unwrap_or(self.visible[0]);
        self.selected = Some(cursor);
        self.anchor = Some(cursor);
    }

    pub fn is_chosen(&self, row: usize) -> bool {
        self.visible
            .get(row)
            .is_some_and(|node| self.chosen.contains(node))
    }

    pub fn chosen_count(&self) -> usize {
        self.chosen.len()
    }

    /// The nodes an action works on, in display order: the chosen set (or the cursor alone
    /// when it is empty), without any node that sits under a chosen folder.
    pub fn picked(&self) -> Vec<u32> {
        let nodes: Vec<u32> = if self.chosen.is_empty() {
            self.selected.into_iter().collect()
        } else {
            self.chosen.iter().copied().collect()
        };
        let set: HashSet<u32> = nodes.iter().copied().collect();
        nodes
            .into_iter()
            .filter(|&n| {
                let mut parent = self.cmp.nodes[n as usize].parent;
                while let Some(p) = parent {
                    if set.contains(&p) {
                        return false;
                    }
                    parent = self.cmp.nodes[p as usize].parent;
                }
                true
            })
            .collect()
    }

    /// What the chips and the name filter show, ignoring whether a folder above is collapsed.
    pub fn passes_filters(&self, node: u32) -> bool {
        let i = node as usize;
        self.subtree[i] & self.filter.mask() != 0
            && (self.name_filter.is_none() || self.name_hit[i])
    }

    /// The chosen nodes' paths, in display order.
    pub fn chosen_rels(&self) -> Vec<&str> {
        self.chosen
            .iter()
            .map(|&n| self.cmp.nodes[n as usize].rel.as_str())
            .collect()
    }

    /// Shows only rows whose relative path contains `text` (any case), with the folders
    /// above them, which open while the filter is on. Blank text turns the filter off.
    pub fn set_name_filter(&mut self, text: &str) {
        let needle = text.trim().to_lowercase();
        if needle.is_empty() {
            self.name_filter = None;
            self.name_hit.clear();
        } else {
            let n = self.cmp.nodes.len();
            let mut hit = vec![false; n];
            // Children come after their parent, so a reverse pass sees them first.
            for i in (0..n).rev() {
                if !hit[i] && self.cmp.nodes[i].rel.to_lowercase().contains(&needle) {
                    hit[i] = true;
                }
                if hit[i]
                    && let Some(parent) = self.cmp.nodes[i].parent
                {
                    hit[parent as usize] = true;
                }
            }
            self.name_hit = hit;
            self.name_filter = Some(needle);
        }
        self.rebuild();
    }

    pub fn name_filter(&self) -> Option<&str> {
        self.name_filter.as_deref()
    }

    pub fn select_first(&mut self) {
        self.select_row(0);
    }

    pub fn select_last(&mut self) {
        if let Some(last) = self.visible.len().checked_sub(1) {
            self.select_row(last);
        }
    }

    pub fn move_selection(&mut self, delta: isize) {
        if self.visible.is_empty() {
            return;
        }
        let row = self.selected_row().map_or(0, |row| {
            row.saturating_add_signed(delta).min(self.visible.len() - 1)
        });
        self.select_row(row);
    }

    pub fn toggle_row(&mut self, row: usize) {
        let node = self.visible[row] as usize;
        if self.layout == Layout::Tree && !self.leaf[node] {
            self.expanded[node] = !self.expanded[node];
            self.rebuild();
        }
    }

    pub fn expand_selected(&mut self) {
        if self.layout == Layout::Flat {
            return;
        }
        if let Some(node) = self.selected.map(|n| n as usize)
            && !self.leaf[node]
        {
            self.expanded[node] = true;
            self.rebuild();
        }
    }

    /// Collapses an open folder; on a file or a closed folder, selects the parent instead.
    pub fn collapse_selected(&mut self) {
        if self.layout == Layout::Flat {
            return;
        }
        let Some(node) = self.selected.map(|n| n as usize) else {
            return;
        };
        if !self.leaf[node] && self.expanded[node] {
            self.expanded[node] = false;
            self.rebuild();
        } else if let Some(parent) = self.cmp.nodes[node].parent {
            self.set_single(Some(parent));
        }
    }

    pub fn expand_all(&mut self) {
        if self.layout == Layout::Flat {
            return;
        }
        for (open, leaf) in self.expanded.iter_mut().zip(&self.leaf) {
            *open = !leaf;
        }
        self.rebuild();
    }

    pub fn collapse_all(&mut self) {
        if self.layout == Layout::Flat {
            return;
        }
        self.expanded.fill(false);
        self.rebuild();
    }

    /// A navigation stop: a differing file, or the top of an orphan/error subtree.
    fn is_stop(&self, i: usize) -> bool {
        let node = &self.cmp.nodes[i];
        match node.status {
            Status::Different { .. } => self.leaf[i],
            Status::LeftOnly | Status::RightOnly | Status::Error(_) => node
                .parent
                .is_none_or(|p| self.cmp.nodes[p as usize].status != node.status),
            Status::Same | Status::Similar { .. } | Status::Pending => false,
        }
    }

    fn reveal(&mut self, index: usize) {
        let mut parent = self.cmp.nodes[index].parent;
        while let Some(p) = parent {
            self.expanded[p as usize] = true;
            parent = self.cmp.nodes[p as usize].parent;
        }
    }

    fn jump(&mut self, candidates: impl Iterator<Item = usize>) -> Option<usize> {
        let mask = self.filter.mask();
        let filtering = self.name_filter.is_some();
        let target = candidates
            .filter(|&i| self.subtree[i] & mask != 0)
            // A row the name filter hides is not a stop: selecting it would snap back.
            .filter(|&i| !filtering || self.name_hit[i])
            .find(|&i| self.is_stop(i))?;
        self.reveal(target);
        self.set_single(Some(target as u32));
        self.rebuild();
        self.selected_row()
    }

    /// Flat list: every differing, one-sided or unreadable file is a stop.
    fn is_flat_stop(&self, row: usize) -> bool {
        !matches!(
            self.cmp.nodes[self.visible[row] as usize].status,
            Status::Same | Status::Similar { .. } | Status::Pending
        )
    }

    /// Flat list: selects the first stop among `rows` (row numbers); returns its row.
    fn jump_rows(&mut self, mut rows: impl Iterator<Item = usize>) -> Option<usize> {
        let row = rows.find(|&row| self.is_flat_stop(row))?;
        self.set_single(Some(self.visible[row]));
        Some(row)
    }

    pub fn next_difference(&mut self) -> Option<usize> {
        if self.layout == Layout::Flat {
            let start = self.selected_row().map_or(0, |row| row + 1);
            return self.jump_rows(start..self.visible.len());
        }
        let start = self.selected.map_or(0, |n| n as usize + 1);
        self.jump(start..self.cmp.nodes.len())
    }

    pub fn prev_difference(&mut self) -> Option<usize> {
        if self.layout == Layout::Flat {
            let end = self.selected_row().unwrap_or(0);
            return self.jump_rows((0..end).rev());
        }
        let end = self.selected.map_or(0, |n| n as usize);
        self.jump((0..end).rev())
    }

    /// F8: the next difference, or the first one again after the last; true when it wrapped.
    pub fn next_difference_wrapping(&mut self) -> Option<(usize, bool)> {
        let before = self.selected;
        if let Some(row) = self.next_difference() {
            return Some((row, false));
        }
        self.selected = None;
        match self.next_difference() {
            Some(row) => Some((row, before.is_some())),
            None => {
                self.selected = before;
                None
            }
        }
    }

    /// F7: the previous difference, or the last one again before the first.
    pub fn prev_difference_wrapping(&mut self) -> Option<(usize, bool)> {
        let before = self.selected;
        if let Some(row) = self.prev_difference() {
            return Some((row, false));
        }
        let last = self.cmp.nodes.len();
        let found = if self.layout == Layout::Flat {
            self.jump_rows((0..self.visible.len()).rev())
        } else {
            self.jump((0..last).rev())
        };
        match found {
            Some(row) => Some((row, before.is_some())),
            None => {
                self.selected = before;
                None
            }
        }
    }

    /// Per-status totals, counted once when the model is built: statuses never change after.
    pub fn counts(&self) -> Counts {
        self.counts
    }

    fn count(cmp: &FolderComparison, leaf: &[bool]) -> Counts {
        let mut counts = Counts::default();
        for (i, node) in cmp.nodes.iter().enumerate().filter(|(i, _)| leaf[*i]) {
            counts.all += 1;
            if node.case_differs() {
                counts.case_names += 1;
            }
            if let Some(entry) = node.left.as_ref().filter(|e| e.kind == EntryKind::File) {
                counts.left_files += 1;
                counts.left_bytes += entry.len;
            }
            if let Some(entry) = node.right.as_ref().filter(|e| e.kind == EntryKind::File) {
                counts.right_files += 1;
                counts.right_bytes += entry.len;
            }
            match node.status {
                Status::Same => {
                    counts.same += 1;
                    counts.time_only += usize::from(cmp.time_differs(i as u32));
                }
                Status::Different { .. } => {
                    counts.different += 1;
                    match node.warning() {
                        Some(Warning::Script) => counts.script += 1,
                        Some(Warning::OddSpace) => counts.odd_space += 1,
                        _ => {}
                    }
                }
                Status::Similar { .. } => {
                    counts.similar += 1;
                    if node.note.as_ref().is_some_and(|note| note.soft) {
                        counts.soft += 1;
                    } else {
                        counts.format_only += 1;
                    }
                    counts.time_only += usize::from(cmp.time_differs(i as u32));
                }
                Status::LeftOnly => counts.left_only += 1,
                Status::RightOnly => counts.right_only += 1,
                Status::Error(_) => counts.errors += 1,
                Status::Pending => counts.pending += 1,
            }
        }
        counts
    }

    /// After a refresh: keep the old filter, folder expansion and selection, matched by path.
    pub fn carry_over(&mut self, old: &FolderModel) {
        let state = |open: bool| -> HashSet<&str> {
            old.cmp
                .nodes
                .iter()
                .zip(&old.expanded)
                .filter(|(node, expanded)| node.is_dir && **expanded == open)
                .map(|(node, _)| node.rel.as_str())
                .collect()
        };
        self.layout = old.layout;
        self.sort_key = old.sort_key;
        self.sort_desc = old.sort_desc;
        let (opened, closed) = (state(true), state(false));
        for (node, expanded) in self.cmp.nodes.iter().zip(self.expanded.iter_mut()) {
            if opened.contains(node.rel.as_str()) {
                *expanded = true;
            } else if closed.contains(node.rel.as_str()) {
                *expanded = false;
            }
        }
        self.filter = old.filter;
        let by_rel: HashMap<&str, u32> = self
            .cmp
            .nodes
            .iter()
            .enumerate()
            .map(|(i, n)| (n.rel.as_str(), i as u32))
            .collect();
        let cursor = old
            .selected_node()
            .and_then(|sel| by_rel.get(sel.rel.as_str()).copied());
        let chosen: BTreeSet<u32> = old
            .chosen_rels()
            .into_iter()
            .filter_map(|rel| by_rel.get(rel).copied())
            .collect();
        self.selected = cursor;
        self.anchor = cursor;
        self.chosen = chosen;
        for node in self
            .chosen
            .iter()
            .copied()
            .chain(cursor)
            .collect::<Vec<_>>()
        {
            self.reveal(node as usize);
        }
        if let Some(filter) = old.name_filter.clone() {
            self.set_name_filter(&filter);
        }
        self.rebuild();
    }
}

#[cfg(test)]
mod tests;
