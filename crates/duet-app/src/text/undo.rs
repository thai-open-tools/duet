//! Undo and redo for one text tab. An entry is one replacement on one side; an edit that
//! continues the open entry (typing on, Backspace over what was typed, an IME recomposing)
//! merges into it until something seals it.

use std::ops::Range;

/// The undo list keeps at most this many entries.
const LIMIT: usize = 10_000;

/// The undo and redo lists hold at most about this much text: a copy-all on a 60 MiB side keeps
/// 120 MiB, so a count alone bounds nothing. The newest entry stays whatever its size.
const BYTE_LIMIT: usize = 256 << 20;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    pub left: bool,
    pub at: usize,
    pub removed: String,
    pub inserted: String,
    /// The caret before the edit; undo puts it back.
    pub caret_before: usize,
}

impl Edit {
    /// Where the inserted text sits once the edit is applied.
    pub fn applied_range(&self) -> Range<usize> {
        self.at..self.at + self.inserted.len()
    }
}

#[derive(Debug)]
pub struct UndoStack {
    done: Vec<Edit>,
    undone: Vec<Edit>,
    open: bool,
    /// Text bytes both lists hold.
    bytes: usize,
    byte_limit: usize,
}

impl Default for UndoStack {
    fn default() -> Self {
        Self::with_byte_limit(BYTE_LIMIT)
    }
}

fn size(edit: &Edit) -> usize {
    edit.removed.len() + edit.inserted.len()
}

fn breaks(s: &str) -> bool {
    s.contains(['\n', '\r'])
}

/// Folds `next` into `last` when it continues it; false leaves both untouched.
fn merge(last: &mut Edit, next: &Edit) -> bool {
    if last.left != next.left || breaks(&next.inserted) || breaks(&last.inserted) {
        return false;
    }
    let end = last.at + last.inserted.len();
    // Replaces a tail of what `last` inserted (typing on is the empty tail).
    if next.at >= last.at
        && next.at + next.removed.len() == end
        && last.inserted.ends_with(next.removed.as_str())
    {
        last.inserted
            .truncate(last.inserted.len() - next.removed.len());
        last.inserted.push_str(&next.inserted);
        return true;
    }
    // Backspace over older text, right before the last deletion.
    if last.inserted.is_empty()
        && next.inserted.is_empty()
        && next.at + next.removed.len() == last.at
        && !breaks(&next.removed)
    {
        last.removed.insert_str(0, &next.removed);
        last.at = next.at;
        return true;
    }
    false
}

impl UndoStack {
    pub fn with_byte_limit(byte_limit: usize) -> Self {
        Self {
            done: Vec::new(),
            undone: Vec::new(),
            open: false,
            bytes: 0,
            byte_limit,
        }
    }

    pub fn record(&mut self, edit: Edit) {
        self.bytes -= self.undone.drain(..).map(|e| size(&e)).sum::<usize>();
        if self.open
            && let Some(last) = self.done.last_mut()
        {
            let before = size(last);
            if merge(last, &edit) {
                self.bytes = self.bytes - before + size(last);
                // A merge can cancel the entry out (typed text backspaced away): drop it, so
                // undo never spends a press on nothing.
                if last.removed.is_empty() && last.inserted.is_empty() {
                    self.done.pop();
                    self.open = false;
                }
                return;
            }
        }
        self.bytes += size(&edit);
        self.done.push(edit);
        // The oldest entries go past either limit; the newest always stays.
        let mut cut = self.done.len().saturating_sub(LIMIT);
        let mut bytes = self.bytes - self.done[..cut].iter().map(size).sum::<usize>();
        while bytes > self.byte_limit && cut + 1 < self.done.len() {
            bytes -= size(&self.done[cut]);
            cut += 1;
        }
        self.done.drain(..cut);
        self.bytes = bytes;
        self.open = true;
    }

    /// The next edit starts a new entry.
    pub fn seal(&mut self) {
        self.open = false;
    }

    pub fn can_undo(&self) -> bool {
        !self.done.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.undone.is_empty()
    }

    pub fn undo(&mut self) -> Option<Edit> {
        self.open = false;
        let edit = self.done.pop()?;
        self.undone.push(edit.clone());
        Some(edit)
    }

    pub fn redo(&mut self) -> Option<Edit> {
        self.open = false;
        let edit = self.undone.pop()?;
        self.done.push(edit.clone());
        Some(edit)
    }

    /// F9: the text that was left is now right.
    pub fn swap_sides(&mut self) {
        for edit in self.done.iter_mut().chain(self.undone.iter_mut()) {
            edit.left = !edit.left;
        }
    }

    pub fn clear(&mut self) {
        *self = Self::with_byte_limit(self.byte_limit);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typed(at: usize, text: &str) -> Edit {
        Edit {
            left: true,
            at,
            removed: String::new(),
            inserted: text.into(),
            caret_before: at,
        }
    }

    #[test]
    fn can_undo_and_redo_follow_the_stack() {
        let mut stack = UndoStack::default();
        assert!(!stack.can_undo() && !stack.can_redo());
        stack.record(typed(0, "a"));
        assert!(stack.can_undo() && !stack.can_redo());
        stack.undo();
        assert!(!stack.can_undo() && stack.can_redo());
        stack.redo();
        assert!(stack.can_undo() && !stack.can_redo());
    }

    #[test]
    fn typing_on_a_line_is_one_step() {
        let mut stack = UndoStack::default();
        for (i, ch) in ["a", "b", "c"].iter().enumerate() {
            stack.record(typed(i, ch));
        }
        let step = stack.undo().unwrap();
        assert_eq!((step.at, step.inserted.as_str()), (0, "abc"));
        assert!(stack.undo().is_none());
        assert_eq!(stack.redo().unwrap().inserted, "abc");
    }

    #[test]
    fn a_seal_or_a_line_break_starts_a_new_step() {
        let mut stack = UndoStack::default();
        stack.record(typed(0, "a"));
        stack.seal();
        stack.record(typed(1, "b"));
        stack.record(typed(2, "\n"));
        stack.record(typed(3, "c"));
        let inserted: Vec<String> = std::iter::from_fn(|| stack.undo())
            .map(|e| e.inserted)
            .collect();
        assert_eq!(inserted, ["c", "\n", "b", "a"]);
    }

    #[test]
    fn backspace_and_recomposition_fold_into_the_typing_step() {
        let mut stack = UndoStack::default();
        stack.record(typed(0, "x"));
        stack.record(typed(1, "か"));
        // The IME replaces its composition "か" with "火".
        stack.record(Edit {
            left: true,
            at: 1,
            removed: "か".into(),
            inserted: "火".into(),
            caret_before: 4,
        });
        // Backspace removes the "火" just typed.
        stack.record(Edit {
            left: true,
            at: 1,
            removed: "火".into(),
            inserted: String::new(),
            caret_before: 4,
        });
        let step = stack.undo().unwrap();
        assert_eq!(
            (step.at, step.removed.as_str(), step.inserted.as_str()),
            (0, "", "x")
        );
        assert!(stack.undo().is_none());
    }

    #[test]
    fn backspaces_over_old_text_join_and_a_new_edit_clears_redo() {
        let mut stack = UndoStack::default();
        let back = |at: usize, gone: &str| Edit {
            left: false,
            at,
            removed: gone.into(),
            inserted: String::new(),
            caret_before: at + gone.len(),
        };
        stack.record(back(4, "d"));
        stack.record(back(3, "c"));
        let step = stack.undo().unwrap();
        assert_eq!((step.at, step.removed.as_str()), (3, "cd"));
        stack.record(typed(0, "z"));
        assert!(stack.redo().is_none(), "a new edit drops the redo list");
    }

    #[test]
    fn swapping_sides_flips_every_entry() {
        let mut stack = UndoStack::default();
        stack.record(typed(0, "a"));
        stack.swap_sides();
        assert!(!stack.undo().unwrap().left);
    }

    /// A replacement of `text` by its upper case: twice its length in bytes.
    fn replaced(at: usize, text: &str) -> Edit {
        Edit {
            left: true,
            at,
            removed: text.into(),
            inserted: text.to_uppercase(),
            caret_before: at,
        }
    }

    fn steps(stack: &mut UndoStack) -> Vec<usize> {
        std::iter::from_fn(|| stack.undo()).map(|e| e.at).collect()
    }

    #[test]
    fn past_the_byte_cap_the_oldest_steps_go_but_never_the_newest() {
        let mut stack = UndoStack::with_byte_limit(20);
        for at in [0, 10, 20] {
            stack.record(replaced(at, "aaaa"));
            stack.seal();
        }
        assert_eq!(
            steps(&mut stack),
            [20, 10],
            "24 bytes: the oldest step went"
        );
        let mut stack = UndoStack::with_byte_limit(4);
        stack.record(replaced(0, "bbbbbbbb"));
        assert_eq!(steps(&mut stack), [0], "one step over the cap still undoes");
    }

    #[test]
    fn the_byte_cap_counts_merged_typing_and_forgets_dropped_redo() {
        let mut stack = UndoStack::with_byte_limit(10);
        for (i, ch) in "abcdefghij".chars().enumerate() {
            stack.record(typed(i, &ch.to_string()));
        }
        stack.seal();
        stack.record(typed(20, "z"));
        assert_eq!(steps(&mut stack), [20], "the merged ten bytes counted");
        let mut stack = UndoStack::with_byte_limit(10);
        stack.record(replaced(0, "aaaa"));
        stack.undo();
        // A new edit drops the undone step and its bytes.
        stack.record(replaced(10, "bbbb"));
        stack.seal();
        stack.record(typed(30, "c"));
        assert_eq!(steps(&mut stack), [30, 10]);
    }

    #[test]
    fn typing_then_backspacing_it_all_away_leaves_no_step() {
        let mut stack = UndoStack::default();
        stack.record(typed(0, "a"));
        stack.record(Edit {
            left: true,
            at: 0,
            removed: "a".into(),
            inserted: String::new(),
            caret_before: 1,
        });
        assert!(stack.undo().is_none(), "the cancelled entry is dropped");
    }
}
