//! Line-level diff over interned line keys. Ignore options change only the comparison keys;
//! displayed text always comes from the original bytes.

use std::borrow::Cow;
use std::ops::Range;

use imara_diff::{Algorithm, Diff, IndentHeuristic, IndentLevel, Token};

use crate::cancel::{CancelToken, Cancelled};
use crate::text::lines::LineIndex;

/// imara-diff asserts below i32::MAX tokens per side; refuse earlier instead of panicking.
pub const MAX_DIFF_LINES: usize = i32::MAX as usize - 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WhitespaceMode {
    #[default]
    None,
    Trailing,
    Leading,
    Edges,
    Amount,
    All,
}

impl WhitespaceMode {
    pub const ALL: [WhitespaceMode; 6] = [
        WhitespaceMode::None,
        WhitespaceMode::Trailing,
        WhitespaceMode::Leading,
        WhitespaceMode::Edges,
        WhitespaceMode::Amount,
        WhitespaceMode::All,
    ];

    /// The word the settings file keeps for this mode.
    pub fn key(self) -> &'static str {
        match self {
            WhitespaceMode::None => "none",
            WhitespaceMode::Trailing => "trailing",
            WhitespaceMode::Leading => "leading",
            WhitespaceMode::Edges => "edges",
            WhitespaceMode::Amount => "amount",
            WhitespaceMode::All => "all",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|mode| mode.key() == key)
    }
}

/// `content` (a line without its terminator) as the whitespace mode compares it. Whitespace is
/// `char::is_whitespace` in every mode.
pub fn normalize_whitespace(content: &str, mode: WhitespaceMode) -> Cow<'_, str> {
    match mode {
        WhitespaceMode::None => Cow::Borrowed(content),
        WhitespaceMode::Trailing => Cow::Borrowed(content.trim_end()),
        WhitespaceMode::Leading => Cow::Borrowed(content.trim_start()),
        WhitespaceMode::Edges => Cow::Borrowed(content.trim()),
        WhitespaceMode::Amount => {
            let mut out = String::with_capacity(content.len());
            for word in content.split_whitespace() {
                if !out.is_empty() {
                    out.push(' ');
                }
                out.push_str(word);
            }
            if out == content {
                Cow::Borrowed(content)
            } else {
                Cow::Owned(out)
            }
        }
        WhitespaceMode::All => {
            if content.chars().any(char::is_whitespace) {
                Cow::Owned(content.chars().filter(|c| !c.is_whitespace()).collect())
            } else {
                Cow::Borrowed(content)
            }
        }
    }
}

/// Paired lines outside hunks and soft runs, plus the paired heads of soft runs, whose text
/// without the line ending differs although their keys matched (whitespace, case, rules).
pub fn count_soft_pairs(left: DiffInput<'_>, right: DiffInput<'_>, diff: &LineDiff) -> u32 {
    count_soft_pairs_checked(left, right, diff, &CancelToken::new()).unwrap_or(0)
}

/// `count_soft_pairs` that stops when `cancel` fires (checked every 65,536 lines).
pub fn count_soft_pairs_checked(
    left: DiffInput<'_>,
    right: DiffInput<'_>,
    diff: &LineDiff,
    cancel: &CancelToken,
) -> Result<u32, Cancelled> {
    let mut count = 0u32;
    let mut scanned = 0u32;
    let mut pairs = |l: u32, r: u32, len: u32| -> Result<(), Cancelled> {
        for k in 0..len {
            scanned += 1;
            if scanned.is_multiple_of(65_536) {
                cancel.check()?;
            }
            let a = left.lines.line_bytes(left.bytes, (l + k) as usize);
            let b = right.lines.line_bytes(right.bytes, (r + k) as usize);
            if a != b {
                count += 1;
            }
        }
        Ok(())
    };
    let (mut l, mut r) = (0u32, 0u32);
    for (soft, run) in diff.ordered() {
        pairs(l, r, run.left.start - l)?;
        if soft {
            let paired = (run.left.end - run.left.start).min(run.right.end - run.right.start);
            pairs(run.left.start, run.right.start, paired)?;
        }
        (l, r) = (run.left.end, run.right.end);
    }
    pairs(l, r, diff.left_lines - l)?;
    Ok(count)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DiffOptions {
    pub whitespace: WhitespaceMode,
    pub ignore_case: bool,
    pub ignore_eol: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct DiffInput<'a> {
    pub bytes: &'a [u8],
    pub lines: &'a LineIndex,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineHunk {
    pub left: Range<u32>,
    pub right: Range<u32>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LineDiff {
    pub hunks: Vec<LineHunk>,
    pub left_lines: u32,
    pub right_lines: u32,
    pub soft: Vec<LineHunk>,
    pub soft_pairs: u32,
}

impl LineDiff {
    pub fn is_identical(&self) -> bool {
        self.hunks.is_empty()
    }

    /// The same diff with the sides exchanged.
    pub fn swapped(&self) -> LineDiff {
        let swap = |runs: &[LineHunk]| {
            runs.iter()
                .map(|h| LineHunk {
                    left: h.right.clone(),
                    right: h.left.clone(),
                })
                .collect()
        };
        LineDiff {
            hunks: swap(&self.hunks),
            left_lines: self.right_lines,
            right_lines: self.left_lines,
            soft: swap(&self.soft),
            soft_pairs: self.soft_pairs,
        }
    }

    /// Hunks and soft runs in file order, each flagged `true` when soft. Ordered by (left start,
    /// right start); a hunk comes first on a tie (PF12). Lazy: a huge diff allocates nothing.
    pub(crate) fn ordered(&self) -> impl Iterator<Item = (bool, &LineHunk)> {
        let key = |h: &LineHunk| (h.left.start, h.right.start);
        let (mut hard, mut soft) = (self.hunks.iter().peekable(), self.soft.iter().peekable());
        std::iter::from_fn(move || match (hard.peek(), soft.peek()) {
            (Some(h), Some(s)) if key(s) < key(h) => soft.next().map(|s| (true, s)),
            (Some(_), _) => hard.next().map(|h| (false, h)),
            (None, Some(_)) => soft.next().map(|s| (true, s)),
            (None, None) => None,
        })
    }

    /// A provisional diff after lines `line..line + old_lines` of one side became `new_lines`
    /// lines: every hunk the edit touches merges with it, later hunks shift. Consistent but not
    /// minimal; the background re-diff replaces it.
    pub fn splice(&self, left: bool, line: u32, old_lines: u32, new_lines: u32) -> LineDiff {
        let side = |h: &LineHunk| {
            if left {
                (h.left.clone(), h.right.clone())
            } else {
                (h.right.clone(), h.left.clone())
            }
        };
        let make = |s: Range<u32>, o: Range<u32>| {
            if left {
                LineHunk { left: s, right: o }
            } else {
                LineHunk { left: o, right: s }
            }
        };
        let edit = line..line + old_lines;
        let delta = i64::from(new_lines) - i64::from(old_lines);
        let shift =
            |r: Range<u32>| (i64::from(r.start) + delta) as u32..(i64::from(r.end) + delta) as u32;
        let mut hunks = Vec::with_capacity(self.hunks.len() + 1);
        let mut soft = Vec::with_capacity(self.soft.len());
        // Offset from an edited-side line to the other side's line in the Same run before it.
        let mut offset = 0i64;
        let mut merged: Option<(Range<u32>, Range<u32>)> = None;
        let mut placed = false;
        // A soft run is spliced like a hunk: one touching the edit joins its hunk.
        for (is_soft, h) in self.ordered() {
            let (s, o) = side(h);
            let keep = |h: LineHunk, hunks: &mut Vec<LineHunk>, soft: &mut Vec<LineHunk>| {
                if is_soft { soft.push(h) } else { hunks.push(h) }
            };
            if s.end < edit.start {
                offset = i64::from(o.end) - i64::from(s.end);
                keep(h.clone(), &mut hunks, &mut soft);
                continue;
            }
            if s.start > edit.end {
                if !placed {
                    hunks.push(Self::edit_hunk(
                        &edit,
                        new_lines,
                        offset,
                        merged.take(),
                        &make,
                    ));
                    placed = true;
                }
                keep(make(shift(s), o), &mut hunks, &mut soft);
                continue;
            }
            // Touching: grow the merged region.
            merged = Some(match merged {
                None => (s, o),
                Some((ms, mo)) => (ms.start..s.end.max(ms.end), mo.start..o.end.max(mo.end)),
            });
        }
        if !placed {
            hunks.push(Self::edit_hunk(&edit, new_lines, offset, merged, &make));
        }
        hunks.retain(|h| !(h.left.is_empty() && h.right.is_empty()));
        soft.retain(|h| !(h.left.is_empty() && h.right.is_empty()));
        let (mut left_lines, mut right_lines) = (self.left_lines, self.right_lines);
        let total = if left {
            &mut left_lines
        } else {
            &mut right_lines
        };
        *total = (i64::from(*total) + delta) as u32;
        LineDiff {
            hunks,
            left_lines,
            right_lines,
            soft,
            soft_pairs: self.soft_pairs,
        }
    }

    /// The hunk for an edit of `edit` (old edited-side lines) into `new_lines` lines, merged
    /// with the touching hunks' region `merged`; `offset` maps a Same-run line across.
    fn edit_hunk(
        edit: &Range<u32>,
        new_lines: u32,
        offset: i64,
        merged: Option<(Range<u32>, Range<u32>)>,
        make: &dyn Fn(Range<u32>, Range<u32>) -> LineHunk,
    ) -> LineHunk {
        let across = |line: u32, offset: i64| (i64::from(line) + offset) as u32;
        let (s, o) = match merged {
            None => (
                edit.clone(),
                across(edit.start, offset)..across(edit.end, offset),
            ),
            Some((ms, mo)) => {
                let s = edit.start.min(ms.start)..edit.end.max(ms.end);
                let o_start = if edit.start < ms.start {
                    across(edit.start, offset)
                } else {
                    mo.start
                };
                let after = i64::from(mo.end) - i64::from(ms.end);
                let o_end = if edit.end > ms.end {
                    across(edit.end, after)
                } else {
                    mo.end
                };
                (s, o_start..o_end)
            }
        };
        let grown = s.start..s.end - (edit.end - edit.start) + new_lines;
        make(grown, o)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DiffError {
    #[error(transparent)]
    Cancelled(#[from] Cancelled),
    #[error("too many lines to diff ({0})")]
    TooManyLines(usize),
}

pub fn diff_lines<'a>(
    left: DiffInput<'a>,
    right: DiffInput<'a>,
    opts: DiffOptions,
    cancel: &CancelToken,
) -> Result<LineDiff, DiffError> {
    diff_core(left, right, opts, cancel, None).map(|(diff, _)| diff)
}

/// `diff_lines` plus the algorithm whose result was kept; `force` skips the adaptive choice.
pub(crate) fn diff_core<'a>(
    left: DiffInput<'a>,
    right: DiffInput<'a>,
    opts: DiffOptions,
    cancel: &CancelToken,
    force: Option<Algorithm>,
) -> Result<(LineDiff, Algorithm), DiffError> {
    diff_core_anchored(left, right, opts, cancel, force, &[])
}

/// `diff_core` with manual alignments: line pairs (left, right) that are forced to pair. The
/// lines between two anchors are diffed on their own, so nothing crosses an anchor; an anchored
/// pair whose keys differ is a one-line hunk of its own (`ldDiffInts`, `de:197-212`). Anchors are
/// sorted by the left line and any that does not come after the previous one on both sides, or
/// lies outside a side, is skipped.
pub(crate) fn diff_core_anchored<'a>(
    left: DiffInput<'a>,
    right: DiffInput<'a>,
    opts: DiffOptions,
    cancel: &CancelToken,
    force: Option<Algorithm>,
    anchors: &[(u32, u32)],
) -> Result<(LineDiff, Algorithm), DiffError> {
    ensure_within_limit(left.lines.len())?;
    ensure_within_limit(right.lines.len())?;
    let tokens = Tokens::build([left, right], opts, cancel)?;
    cancel.check()?;
    let indent = &tokens.indent;
    let first = force.unwrap_or(Algorithm::Myers);
    let mut used = first;
    let mut hunks = Vec::new();
    // Diffs lines `a` of the left tokens against lines `b` of the right ones into `out`.
    let segment =
        |a: Range<usize>, b: Range<usize>, out: &mut Vec<LineHunk>, used: &mut Algorithm| {
            if a.is_empty() && b.is_empty() {
                return;
            }
            let (before, after) = (&tokens.before[a.clone()], &tokens.after[b.clone()]);
            let mut diff = Diff::default();
            // Myers is near-linear on real inputs; Histogram reads better but is O(N·D) when most
            // lines are unique, so it only runs when Myers shows the work is small.
            diff.compute_with(first, before, after, tokens.count);
            if force.is_none() && use_histogram(before.len() + after.len(), diff.hunks().count()) {
                diff.compute_with(Algorithm::Histogram, before, after, tokens.count);
                *used = Algorithm::Histogram;
            }
            diff.postprocess_with(
                before,
                after,
                IndentHeuristic::new(|token: Token| indent[token.0 as usize]),
            );
            out.extend(diff.hunks().map(|h| LineHunk {
                left: h.before.start + a.start as u32..h.before.end + a.start as u32,
                right: h.after.start + b.start as u32..h.after.end + b.start as u32,
            }));
        };
    let (mut at_left, mut at_right) = (0usize, 0usize);
    for (l, r) in usable_anchors(anchors, tokens.before.len(), tokens.after.len()) {
        segment(at_left..l, at_right..r, &mut hunks, &mut used);
        if tokens.before[l] != tokens.after[r] {
            hunks.push(LineHunk {
                left: l as u32..l as u32 + 1,
                right: r as u32..r as u32 + 1,
            });
        }
        (at_left, at_right) = (l + 1, r + 1);
        cancel.check()?;
    }
    segment(
        at_left..tokens.before.len(),
        at_right..tokens.after.len(),
        &mut hunks,
        &mut used,
    );
    cancel.check()?;
    Ok((
        LineDiff {
            hunks,
            left_lines: left.lines.len() as u32,
            right_lines: right.lines.len() as u32,
            ..LineDiff::default()
        },
        used,
    ))
}

/// The anchors that can apply: sorted by the left line, each after the one before on both sides
/// and inside both sides.
fn usable_anchors(
    anchors: &[(u32, u32)],
    left_lines: usize,
    right_lines: usize,
) -> impl Iterator<Item = (usize, usize)> {
    let mut sorted = anchors.to_vec();
    sorted.sort_by_key(|&(l, _)| l);
    let mut next = (0usize, 0usize);
    sorted.into_iter().filter_map(move |(l, r)| {
        let (l, r) = (l as usize, r as usize);
        let fits = l >= next.0 && r >= next.1 && l < left_lines && r < right_lines;
        if fits {
            next = (l + 1, r + 1);
        }
        fits.then_some((l, r))
    })
}

/// Above this many (lines × Myers hunks), Histogram's rescans cost more than its nicer output.
const HISTOGRAM_BUDGET: u64 = 50_000_000;

pub(crate) fn use_histogram(lines: usize, hunks: usize) -> bool {
    hunks > 0 && (lines as u64).saturating_mul(hunks as u64) <= HISTOGRAM_BUDGET
}

/// One token per line plus the indent level of each distinct key (for the slider heuristic).
struct Tokens {
    before: Vec<Token>,
    after: Vec<Token>,
    indent: Vec<IndentLevel>,
    count: u32,
}

impl Tokens {
    fn build(
        sides: [DiffInput<'_>; 2],
        opts: DiffOptions,
        cancel: &CancelToken,
    ) -> Result<Self, Cancelled> {
        let total = sides[0].lines.len() + sides[1].lines.len();
        let mut interner = LineInterner::new(sides, opts, total, hash_key);
        let before = interner.intern_side(0, &[], cancel)?;
        let after = interner.intern_side(1, &before, cancel)?;
        // The table and representatives are dropped here, before the diff allocates.
        Ok(Self {
            before,
            after,
            count: interner.indent.len() as u32,
            indent: interner.indent,
        })
    }
}

/// Interns comparison keys without storing them: an open-addressing table of token ids and,
/// per token, the line that first produced it. Keys are recomputed from the source bytes to
/// confirm a hash match, so equal hashes of different lines stay different tokens.
struct LineInterner<'a> {
    sides: [DiffInput<'a>; 2],
    opts: DiffOptions,
    hash: fn(&[u8]) -> u64,
    /// Each slot packs token id + 1 in the low `token_bits` and a hash fingerprint above it;
    /// 0 marks an empty slot. Sized once (load <= 3/4), so it never grows.
    table: Vec<u32>,
    mask: usize,
    token_bits: u32,
    token_mask: u32,
    /// Per token: line number with the side in the top bit.
    reps: Vec<u32>,
    indent: Vec<IndentLevel>,
}

const SIDE_BIT: u32 = 1 << 31;

impl<'a> LineInterner<'a> {
    fn new(
        sides: [DiffInput<'a>; 2],
        opts: DiffOptions,
        lines: usize,
        hash: fn(&[u8]) -> u64,
    ) -> Self {
        let slots = (lines + lines / 3 + 1).next_power_of_two().max(16);
        // Token id + 1 never exceeds `lines`; the remaining high bits hold the fingerprint.
        let token_bits = (u32::BITS - (lines as u32).leading_zeros()).max(1);
        Self {
            sides,
            opts,
            hash,
            table: vec![0; slots],
            mask: slots - 1,
            token_bits,
            token_mask: u32::MAX.checked_shr(u32::BITS - token_bits).unwrap_or(0),
            reps: Vec::new(),
            indent: Vec::new(),
        }
    }

    fn key(&self, rep: u32) -> Cow<'a, [u8]> {
        let side = usize::from(rep & SIDE_BIT != 0);
        line_key(self.sides[side], (rep & !SIDE_BIT) as usize, self.opts)
    }

    fn fingerprint(&self, hash: u64) -> u32 {
        ((hash >> 32) as u32)
            .checked_shr(self.token_bits)
            .unwrap_or(0)
    }

    /// `follow` holds the left side's tokens when interning the right side: most right lines
    /// equal the left line after the previous match, which is checked before the table.
    fn intern_side(
        &mut self,
        side: usize,
        follow: &[Token],
        cancel: &CancelToken,
    ) -> Result<Vec<Token>, Cancelled> {
        let side_bit = if side == 1 { SIDE_BIT } else { 0 };
        if follow.is_empty() {
            self.intern_batched(side, side_bit, cancel)
        } else {
            self.intern_following(side, side_bit, follow, cancel)
        }
    }

    /// Builds each key once, hashes a batch and prefetches its table slots, so the cache and
    /// TLB misses on the (large) table overlap instead of stalling one line at a time.
    fn intern_batched(
        &mut self,
        side: usize,
        side_bit: u32,
        cancel: &CancelToken,
    ) -> Result<Vec<Token>, Cancelled> {
        const BATCH: usize = 32;
        let lines = self.sides[side].lines.len();
        let mut tokens = Vec::with_capacity(lines);
        let mut keys: Vec<(Cow<'a, [u8]>, u64)> = Vec::with_capacity(BATCH);
        for start in (0..lines).step_by(BATCH) {
            if start % 65_536 == 0 {
                cancel.check()?;
            }
            keys.clear();
            for line in start..(start + BATCH).min(lines) {
                let key = self.key(line as u32 | side_bit);
                let hash = (self.hash)(&key);
                prefetch(&self.table[hash as usize & self.mask]);
                keys.push((key, hash));
            }
            for (offset, (key, hash)) in keys.iter().enumerate() {
                let rep = (start + offset) as u32 | side_bit;
                tokens.push(Token(self.intern(rep, key, *hash)));
            }
        }
        Ok(tokens)
    }

    fn intern_following(
        &mut self,
        side: usize,
        side_bit: u32,
        follow: &[Token],
        cancel: &CancelToken,
    ) -> Result<Vec<Token>, Cancelled> {
        let (this, left) = (self.sides[side], self.sides[0]);
        let lines = this.lines.len();
        let mut tokens = Vec::with_capacity(lines);
        let mut cursor = 0usize;
        for line in 0..lines {
            if line % 65_536 == 0 {
                cancel.check()?;
            }
            // Byte-identical lines (terminator included) have equal keys under every option,
            // so this check needs no key and no table access.
            if cursor < follow.len()
                && this.lines.line_bytes_with_eol(this.bytes, line)
                    == left.lines.line_bytes_with_eol(left.bytes, cursor)
            {
                tokens.push(follow[cursor]);
                cursor += 1;
                continue;
            }
            let rep = line as u32 | side_bit;
            let key = self.key(rep);
            let token = self.intern(rep, &key, (self.hash)(&key));
            let first = self.reps[token as usize];
            if first & SIDE_BIT == 0 {
                cursor = first as usize + 1;
            }
            tokens.push(Token(token));
        }
        Ok(tokens)
    }

    fn intern(&mut self, rep: u32, key: &[u8], hash: u64) -> u32 {
        let fingerprint = self.fingerprint(hash);
        let mut slot = hash as usize & self.mask;
        loop {
            let stored = self.table[slot];
            if stored == 0 {
                let token = self.reps.len() as u32;
                self.table[slot] =
                    (token + 1) | fingerprint.checked_shl(self.token_bits).unwrap_or(0);
                self.reps.push(rep);
                self.indent
                    .push(IndentLevel::for_ascii_line(key.iter().copied(), 8));
                return token;
            }
            // The fingerprint skips most probes without rebuilding the stored line's key.
            if stored.checked_shr(self.token_bits).unwrap_or(0) == fingerprint {
                let token = (stored & self.token_mask) - 1;
                if *self.key(self.reps[token as usize]) == *key {
                    return token;
                }
            }
            slot = (slot + 1) & self.mask;
        }
    }
}

#[inline(always)]
fn prefetch(slot: &u32) {
    #[cfg(target_arch = "x86_64")]
    {
        use std::arch::x86_64::{_MM_HINT_T0, _mm_prefetch};
        // SAFETY: a prefetch is a hint; it never faults and `slot` is a valid reference.
        unsafe { _mm_prefetch::<_MM_HINT_T0>(std::ptr::from_ref(slot).cast()) };
    }
    #[cfg(not(target_arch = "x86_64"))]
    let _ = slot;
}

/// 64-bit multiply-rotate hash over 8-byte words with a final avalanche, so the low bits
/// used for the table index depend on every input byte.
fn hash_key(bytes: &[u8]) -> u64 {
    const K: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut h = (bytes.len() as u64) ^ K;
    let mut words = bytes.chunks_exact(8);
    for word in &mut words {
        let w = u64::from_le_bytes(word.try_into().expect("8-byte chunk"));
        h = (h.rotate_left(5) ^ w).wrapping_mul(K);
    }
    let rest = words.remainder();
    if !rest.is_empty() {
        let mut buf = [0u8; 8];
        buf[..rest.len()].copy_from_slice(rest);
        h = (h.rotate_left(5) ^ u64::from_le_bytes(buf)).wrapping_mul(K);
    }
    h ^= h >> 33;
    h = h.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    h ^ (h >> 33)
}

fn ensure_within_limit(lines: usize) -> Result<(), DiffError> {
    if lines > MAX_DIFF_LINES {
        Err(DiffError::TooManyLines(lines))
    } else {
        Ok(())
    }
}

/// Comparison key for one line: normalized content plus the terminator unless EOLs are ignored.
pub(crate) fn line_key<'a>(side: DiffInput<'a>, line: usize, opts: DiffOptions) -> Cow<'a, [u8]> {
    if opts.whitespace == WhitespaceMode::None && !opts.ignore_case {
        return Cow::Borrowed(if opts.ignore_eol {
            side.lines.line_bytes(side.bytes, line)
        } else {
            side.lines.line_bytes_with_eol(side.bytes, line)
        });
    }
    let content = String::from_utf8_lossy(side.lines.line_bytes(side.bytes, line));
    let eol: &[u8] = if opts.ignore_eol {
        &[]
    } else {
        side.lines.eol_bytes(side.bytes, line)
    };
    // Sized up front: growing a key while filtering costs several reallocations per line.
    let normalized: Cow<'_, str> = match opts.whitespace {
        WhitespaceMode::All => {
            let mut kept = String::with_capacity(content.len() + eol.len());
            kept.extend(content.chars().filter(|c| !c.is_whitespace()));
            Cow::Owned(kept)
        }
        mode => normalize_whitespace(&content, mode),
    };
    let mut key = if opts.ignore_case {
        normalized.to_lowercase()
    } else {
        normalized.into_owned()
    };
    key.reserve_exact(eol.len());
    let mut bytes = key.into_bytes();
    bytes.extend_from_slice(eol);
    Cow::Owned(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hunk(l: Range<u32>, r: Range<u32>) -> LineHunk {
        LineHunk { left: l, right: r }
    }

    #[test]
    fn typing_in_a_same_run_makes_that_line_a_hunk() {
        let diff = LineDiff {
            hunks: vec![hunk(5..6, 5..6)],
            left_lines: 10,
            right_lines: 10,
            ..LineDiff::default()
        };
        let spliced = diff.splice(true, 2, 1, 1);
        assert_eq!(spliced.hunks, vec![hunk(2..3, 2..3), hunk(5..6, 5..6)]);
        assert_eq!((spliced.left_lines, spliced.right_lines), (10, 10));
    }

    #[test]
    fn inserted_lines_shift_later_hunks_and_touching_hunks_merge() {
        let diff = LineDiff {
            hunks: vec![hunk(2..3, 2..2), hunk(6..7, 5..7)],
            left_lines: 10,
            right_lines: 10,
            ..LineDiff::default()
        };
        // Right line 1 becomes three lines. Hunk 0 (empty on the right at line 2) touches the
        // edit and merges; the hunk after it shifts by +2 on the right.
        let grown = diff.splice(false, 1, 1, 3);
        assert_eq!(grown.right_lines, 12);
        assert_eq!(
            grown.hunks.last().unwrap(),
            &hunk(6..7, 7..9),
            "shifted by +2 on the right"
        );
        let merged = diff.splice(true, 2, 5, 1);
        assert_eq!(
            merged.hunks,
            vec![hunk(2..3, 2..7)],
            "both hunks merged into the edit"
        );
        assert_eq!(merged.left_lines, 6);
    }

    proptest::proptest! {
        #[test]
        fn a_spliced_diff_is_always_consistent(
            left in "[abc\n]{0,30}",
            right in "[abc\n]{0,30}",
            edit_left in proptest::bool::ANY,
            at in 0u32..12,
            old in 0u32..4,
            new in 0u32..4,
        ) {
            let diff = run(&left, &right, DiffOptions::default());
            let total = if edit_left { diff.left_lines } else { diff.right_lines };
            let line = at.min(total);
            let old = old.min(total - line);
            let s = diff.splice(edit_left, line, old, new);
            let (st, ot) = if edit_left { (s.left_lines, s.right_lines) } else { (s.right_lines, s.left_lines) };
            let (bt, bo) = if edit_left { (diff.left_lines, diff.right_lines) } else { (diff.right_lines, diff.left_lines) };
            proptest::prop_assert_eq!(st, bt - old + new);
            proptest::prop_assert_eq!(ot, bo);
            let mut prev = (0u32, 0u32);
            for h in &s.hunks {
                proptest::prop_assert!(h.left.start >= prev.0 && h.right.start >= prev.1);
                proptest::prop_assert!(h.left.end <= s.left_lines && h.right.end <= s.right_lines);
                prev = (h.left.end, h.right.end);
            }
            let rows = crate::text::rows::RowMap::new(&s);
            let _ = rows.change_blocks();
        }
    }

    fn run(left: &str, right: &str, opts: DiffOptions) -> LineDiff {
        let (li, ri) = (
            LineIndex::new(left.as_bytes()),
            LineIndex::new(right.as_bytes()),
        );
        diff_lines(
            DiffInput {
                bytes: left.as_bytes(),
                lines: &li,
            },
            DiffInput {
                bytes: right.as_bytes(),
                lines: &ri,
            },
            opts,
            &CancelToken::new(),
        )
        .unwrap()
    }

    fn hunks(d: &LineDiff) -> Vec<(Range<u32>, Range<u32>)> {
        d.hunks
            .iter()
            .map(|h| (h.left.clone(), h.right.clone()))
            .collect()
    }

    const PLAIN: DiffOptions = DiffOptions {
        whitespace: WhitespaceMode::None,
        ignore_case: false,
        ignore_eol: false,
    };

    #[test]
    fn identical_inputs_have_no_hunks() {
        let d = run("a\nb\n", "a\nb\n", PLAIN);
        assert!(d.is_identical());
        assert_eq!((d.left_lines, d.right_lines), (2, 2));
    }

    #[test]
    fn changed_middle_line() {
        assert_eq!(
            hunks(&run("a\nb\nc\n", "a\nB\nc\n", PLAIN)),
            vec![(1..2, 1..2)]
        );
    }

    #[test]
    fn insertion_and_deletion() {
        assert_eq!(hunks(&run("a\n", "a\nb\n", PLAIN)), vec![(1..1, 1..2)]);
        assert_eq!(hunks(&run("x\na\n", "a\n", PLAIN)), vec![(0..1, 0..0)]);
    }

    #[test]
    fn whitespace_modes() {
        let edges = DiffOptions {
            whitespace: WhitespaceMode::Edges,
            ..PLAIN
        };
        let all = DiffOptions {
            whitespace: WhitespaceMode::All,
            ..PLAIN
        };
        assert!(!run("  a \n", "a\n", PLAIN).is_identical());
        assert!(run("  a \n", "a\n", edges).is_identical());
        assert!(!run("a b\n", "ab\n", edges).is_identical());
        assert!(run("a b\n", "ab\n", all).is_identical());
    }

    #[test]
    fn ignore_case() {
        let opts = DiffOptions {
            ignore_case: true,
            ..PLAIN
        };
        assert!(!run("ABC\n", "abc\n", PLAIN).is_identical());
        assert!(run("ABC\n", "abc\n", opts).is_identical());
    }

    #[test]
    fn line_endings_count_unless_ignored() {
        let ignore = DiffOptions {
            ignore_eol: true,
            ..PLAIN
        };
        assert_eq!(
            hunks(&run("a\r\nb\r\n", "a\nb\n", PLAIN)),
            vec![(0..2, 0..2)]
        );
        assert!(run("a\r\nb\r\n", "a\nb\n", ignore).is_identical());
    }

    #[test]
    fn missing_final_newline_is_a_difference() {
        let ignore = DiffOptions {
            ignore_eol: true,
            ..PLAIN
        };
        assert_eq!(hunks(&run("a\nb", "a\nb\n", PLAIN)), vec![(1..2, 1..2)]);
        assert!(run("a\nb", "a\nb\n", ignore).is_identical());
    }

    #[test]
    fn whitespace_mode_keeps_line_ending_differences() {
        let edges = DiffOptions {
            whitespace: WhitespaceMode::Edges,
            ..PLAIN
        };
        assert!(!run("a \r\n", "a\n", edges).is_identical());
    }

    #[test]
    fn thai_lines() {
        assert_eq!(
            hunks(&run("สวัสดี\nครับ\n", "สวัสดี\nค่ะ\n", PLAIN)),
            vec![(1..2, 1..2)]
        );
    }

    #[test]
    fn cancellation_is_reported() {
        let text = "a\n";
        let index = LineIndex::new(text.as_bytes());
        let input = DiffInput {
            bytes: text.as_bytes(),
            lines: &index,
        };
        let cancel = CancelToken::new();
        cancel.cancel();
        assert_eq!(
            diff_lines(input, input, PLAIN, &cancel),
            Err(DiffError::Cancelled(Cancelled))
        );
    }

    #[test]
    fn line_limit_is_enforced() {
        assert_eq!(ensure_within_limit(MAX_DIFF_LINES), Ok(()));
        assert_eq!(
            ensure_within_limit(MAX_DIFF_LINES + 1),
            Err(DiffError::TooManyLines(MAX_DIFF_LINES + 1))
        );
    }

    #[test]
    fn histogram_runs_only_when_myers_shows_little_work() {
        assert!(
            !use_histogram(10_000, 0),
            "identical input needs no second pass"
        );
        assert!(use_histogram(10_000, 100));
        assert!(
            !use_histogram(300_000, 1_457),
            "the 10 MB benchmark pair takes the Myers path"
        );
    }

    fn run_with(left: &str, right: &str, force: Option<Algorithm>) -> (LineDiff, Algorithm) {
        let (li, ri) = (
            LineIndex::new(left.as_bytes()),
            LineIndex::new(right.as_bytes()),
        );
        diff_core(
            DiffInput {
                bytes: left.as_bytes(),
                lines: &li,
            },
            DiffInput {
                bytes: right.as_bytes(),
                lines: &ri,
            },
            PLAIN,
            &CancelToken::new(),
            force,
        )
        .unwrap()
    }

    /// Applies the hunks to the left lines and checks that the right lines come out.
    fn rebuilds_right(left: &str, right: &str, d: &LineDiff) -> bool {
        let (l, r): (Vec<&str>, Vec<&str>) = (left.lines().collect(), right.lines().collect());
        let mut rebuilt = Vec::new();
        let mut pos = 0usize;
        for h in &d.hunks {
            rebuilt.extend_from_slice(&l[pos..h.left.start as usize]);
            rebuilt.extend_from_slice(&r[h.right.start as usize..h.right.end as usize]);
            pos = h.left.end as usize;
        }
        rebuilt.extend_from_slice(&l[pos..]);
        rebuilt == r
    }

    #[test]
    fn small_inputs_keep_histograms_output() {
        // Myers and Histogram disagree on this pair; normal-sized files must get Histogram's.
        let (left, right) = ("\n\n\n\n\nb\n", "\nc\n\n}\n\n\n");
        let (d, algorithm) = run_with(left, right, None);
        assert_eq!(algorithm, Algorithm::Histogram);
        assert_eq!(hunks(&d), vec![(1..1, 1..4), (3..6, 6..6)]);
        let (myers, _) = run_with(left, right, Some(Algorithm::Myers));
        assert_ne!(
            hunks(&myers),
            hunks(&d),
            "the pair must tell the algorithms apart"
        );
    }

    #[test]
    fn colliding_hashes_keep_different_lines_apart() {
        let text = "alpha\nbeta\nalpha\ngamma\n";
        let index = LineIndex::new(text.as_bytes());
        let side = DiffInput {
            bytes: text.as_bytes(),
            lines: &index,
        };
        let mut interner = LineInterner::new([side, side], PLAIN, 8, |_| 42);
        let tokens = interner.intern_side(0, &[], &CancelToken::new()).unwrap();
        let ids: Vec<u32> = tokens.iter().map(|t| t.0).collect();
        assert_eq!(ids, vec![0, 1, 0, 2]);
        let other = interner
            .intern_side(1, &tokens, &CancelToken::new())
            .unwrap();
        assert_eq!(other.iter().map(|t| t.0).collect::<Vec<_>>(), ids);
    }

    #[test]
    fn large_unique_line_diff_takes_myers_and_still_rebuilds_the_right_side() {
        let (mut left, mut right) = (String::new(), String::new());
        for i in 0..100_000u64 {
            let line = format!("{i:08} {:016x}\n", i.wrapping_mul(0x9E37_79B9_7F4A_7C15));
            left.push_str(&line);
            match i % 100 {
                0 => right.push_str(&format!("{i:08} changed\n")),
                50 => {
                    right.push_str("inserted\n");
                    right.push_str(&line);
                }
                99 => {}
                _ => right.push_str(&line),
            }
        }
        let (d, algorithm) = run_with(&left, &right, None);
        assert_eq!(algorithm, Algorithm::Myers);
        assert!(rebuilds_right(&left, &right, &d));
        // Each deletion at i % 100 == 99 touches the next block's change, so they merge.
        assert_eq!(d.hunks.len(), 2_001);
    }

    proptest::proptest! {
        #[test]
        fn forced_myers_rebuilds_the_right_side(
            left in proptest::collection::vec("[ab]{0,2}", 0..40),
            right in proptest::collection::vec("[ab]{0,2}", 0..40),
        ) {
            let (l, r) = (left.join("\n") + "\n", right.join("\n") + "\n");
            let (d, algorithm) = run_with(&l, &r, Some(Algorithm::Myers));
            proptest::prop_assert_eq!(algorithm, Algorithm::Myers);
            proptest::prop_assert!(rebuilds_right(&l, &r, &d));
        }
    }

    #[test]
    fn right_side_prediction_gives_the_same_tokens_as_the_table() {
        let left = "a\nb\na\nb\nc\n";
        let right = "b\na\nb\na\nd\nc\nb\n";
        let (li, ri) = (
            LineIndex::new(left.as_bytes()),
            LineIndex::new(right.as_bytes()),
        );
        let sides = [
            DiffInput {
                bytes: left.as_bytes(),
                lines: &li,
            },
            DiffInput {
                bytes: right.as_bytes(),
                lines: &ri,
            },
        ];
        let cancel = CancelToken::new();
        let mut predicted = LineInterner::new(sides, PLAIN, 12, hash_key);
        let before = predicted.intern_side(0, &[], &cancel).unwrap();
        let with_follow = predicted.intern_side(1, &before, &cancel).unwrap();
        let mut plain = LineInterner::new(sides, PLAIN, 12, hash_key);
        plain.intern_side(0, &[], &cancel).unwrap();
        let without = plain.intern_side(1, &[], &cancel).unwrap();
        let ids = |t: &[Token]| t.iter().map(|t| t.0).collect::<Vec<_>>();
        assert_eq!(ids(&with_follow), ids(&without));
        assert_eq!(ids(&with_follow), vec![1, 0, 1, 0, 3, 2, 1]);
    }

    #[test]
    fn prediction_falls_back_to_the_table_when_only_ignored_bytes_differ() {
        let opts = DiffOptions {
            whitespace: WhitespaceMode::All,
            ..PLAIN
        };
        let (left, right) = ("a b\nc\n", "ab\nc\n");
        let (li, ri) = (
            LineIndex::new(left.as_bytes()),
            LineIndex::new(right.as_bytes()),
        );
        let sides = [
            DiffInput {
                bytes: left.as_bytes(),
                lines: &li,
            },
            DiffInput {
                bytes: right.as_bytes(),
                lines: &ri,
            },
        ];
        let cancel = CancelToken::new();
        let mut interner = LineInterner::new(sides, opts, 4, hash_key);
        let before = interner.intern_side(0, &[], &cancel).unwrap();
        let after = interner.intern_side(1, &before, &cancel).unwrap();
        assert_eq!(
            after, before,
            "\"a b\" and \"ab\" are one key when all whitespace is ignored"
        );
    }

    #[test]
    fn the_six_whitespace_modes_build_their_keys() {
        let s = " a  b \t";
        let cases = [
            (WhitespaceMode::None, " a  b \t"),
            (WhitespaceMode::Trailing, " a  b"),
            (WhitespaceMode::Leading, "a  b \t"),
            (WhitespaceMode::Edges, "a  b"),
            (WhitespaceMode::Amount, "a b"),
            (WhitespaceMode::All, "ab"),
        ];
        for (mode, want) in cases {
            assert_eq!(normalize_whitespace(s, mode), want, "{mode:?}");
        }
        assert_eq!(
            normalize_whitespace("\u{a0}x\u{3000}", WhitespaceMode::Edges),
            "x"
        );
        for mode in WhitespaceMode::ALL {
            assert_eq!(WhitespaceMode::from_key(mode.key()), Some(mode));
        }
        assert_eq!(WhitespaceMode::from_key("bogus"), None);
        let keys: Vec<&str> = WhitespaceMode::ALL.iter().map(|m| m.key()).collect();
        assert_eq!(
            keys,
            ["none", "trailing", "leading", "edges", "amount", "all"]
        );
    }

    #[test]
    fn whitespace_modes_hide_their_differences() {
        let count = |l: &str, r: &str, whitespace| {
            run(
                l,
                r,
                DiffOptions {
                    whitespace,
                    ..PLAIN
                },
            )
            .hunks
            .len()
        };
        assert_eq!(count("a \n", "a\n", WhitespaceMode::Trailing), 0);
        assert_eq!(count("a \n", "a\n", WhitespaceMode::None), 1);
        assert_eq!(count("  a\n", "a\n", WhitespaceMode::Trailing), 1);
        assert_eq!(count("  a\n", "a\n", WhitespaceMode::Leading), 0);
        assert_eq!(count("a  b\n", "a b\n", WhitespaceMode::Amount), 0);
        assert_eq!(count("a  b\n", "a b\n", WhitespaceMode::Edges), 1);
    }

    #[test]
    fn soft_runs_shift_join_and_swap_with_the_diff() {
        let diff = LineDiff {
            hunks: vec![hunk(5..6, 5..6)],
            soft: vec![hunk(1..3, 1..2)],
            left_lines: 8,
            right_lines: 7,
            ..LineDiff::default()
        };
        let swapped = diff.swapped();
        assert_eq!(swapped.soft, vec![hunk(1..2, 1..3)]);
        assert_eq!(swapped.swapped(), diff);
        let joined = diff.splice(true, 2, 1, 1);
        assert_eq!(joined.hunks, vec![hunk(1..3, 1..2), hunk(5..6, 5..6)]);
        assert!(joined.soft.is_empty());
        let kept = diff.splice(true, 7, 0, 1);
        assert_eq!(kept.soft, vec![hunk(1..3, 1..2)]);
        assert_eq!(kept.hunks, vec![hunk(5..6, 5..6), hunk(7..8, 7..7)]);
        // A soft run after the edit shifts like a hunk.
        let shifted = diff.splice(true, 0, 0, 2);
        assert_eq!(shifted.soft, vec![hunk(3..5, 1..2)]);
        assert_eq!(shifted.hunks, vec![hunk(0..2, 0..0), hunk(7..8, 5..6)]);
    }

    #[test]
    fn soft_pairs_count_what_the_old_options_hid() {
        let counted = |l: &str, r: &str, opts| {
            let d = run(l, r, opts);
            let (li, ri) = (LineIndex::new(l.as_bytes()), LineIndex::new(r.as_bytes()));
            let n = count_soft_pairs(
                DiffInput {
                    bytes: l.as_bytes(),
                    lines: &li,
                },
                DiffInput {
                    bytes: r.as_bytes(),
                    lines: &ri,
                },
                &d,
            );
            (d, n)
        };
        let case = DiffOptions {
            ignore_case: true,
            ..PLAIN
        };
        let (d, n) = counted("A\nb\n", "a\nb\n", case);
        assert!(d.hunks.is_empty());
        assert_eq!(n, 1);
        assert_eq!(d.soft_pairs, 0, "diff_lines itself never counts (PF6)");
        for (l, r) in [("A\nb\n", "a\nb\n"), ("x\ny\n", "x\nz\n"), ("a\r\n", "a\n")] {
            assert_eq!(counted(l, r, PLAIN).1, 0, "{l:?} {r:?}");
        }
        let eol = DiffOptions {
            ignore_eol: true,
            ..PLAIN
        };
        assert_eq!(counted("a\r\nb\n", "a\nb\n", eol).1, 0);
        let ws = DiffOptions {
            whitespace: WhitespaceMode::Edges,
            ..PLAIN
        };
        assert_eq!(counted("a \n b\nc\n", "a\nb\nd\n", ws).1, 2);
    }

    #[test]
    fn a_swapped_diff_mirrors_the_hunks() {
        let diff = LineDiff {
            hunks: vec![LineHunk {
                left: 1..3,
                right: 1..2,
            }],
            left_lines: 5,
            right_lines: 4,
            ..LineDiff::default()
        };
        let swapped = diff.swapped();
        assert_eq!(
            swapped.hunks,
            vec![LineHunk {
                left: 1..2,
                right: 1..3
            }]
        );
        assert_eq!((swapped.left_lines, swapped.right_lines), (4, 5));
        assert_eq!(swapped.swapped(), diff);
    }
}
