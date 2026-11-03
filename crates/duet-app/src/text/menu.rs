//! The right-click menu on text rows (P4 task 10, rulings R7 and R12): what it offers for the
//! row that was pressed, and the handlers of the actions it adds. An `impl TextView` split out
//! of view.rs so the view itself stays readable.

use std::path::PathBuf;

use duet_core::kind::CompareKind;
use gpui_kit::{ClipboardItem, Context, InteractiveElement, Window, actions};

use crate::text::caret::Caret;
use crate::text::model::DisplayRow;
use crate::text::rowsel::RowSel;
use crate::text::view::{BINARY_HINT, TextEvent};
use crate::text::view::{
    CopyBlockHereLeft, CopyBlockHereRight, CopyBlockLeft, CopyBlockRight, CopyLeftPath,
    CopyRightPath, CopyText, DeleteRows, EditRow, OpenLeftExternally, OpenLeftFile,
    OpenRightExternally, OpenRightFile, PasteSideLeft, PasteSideRight, RevealLeftFile,
    RevealRightFile, SaveLeft, SaveLeftAs, SaveRight, SaveRightAs, TextView,
};
use crate::ui::menu::MenuEntry;

actions!(
    text_menu,
    [
        OpenInHex,
        OpenAsTable,
        OpenAsPicture,
        TreatAsUnimportant,
        AlignWithOtherSide,
        ClearAlignments
    ]
);

/// The menu of the row last right-clicked; empty (so no menu opens) when none was, as on a fold
/// row or blank space.
pub fn row_menu(view: &TextView) -> Vec<MenuEntry> {
    let (Some((row, side)), Some(model)) = (view.menu_row, view.model.as_ref()) else {
        return Vec::new();
    };
    let Some(aligned) = model.aligned_row(row) else {
        return Vec::new();
    };
    let can = |left| model.editable(left).is_ok();
    let in_block = model.blocks().iter().any(|b| b.contains(&row));
    // The side the selection is on; a press always made one, so this is only a fallback.
    let (sel_left, rows) = view
        .row_sel
        .map_or((side, row..row + 1), |s| (s.left, s.rows()));
    let placeholder = if side {
        aligned.left.is_none()
    } else {
        aligned.right.is_none()
    };
    let has_lines = !model.side_lines(rows, sel_left).is_empty();
    vec![
        MenuEntry::action("Copy selected rows to the right", CopyBlockRight).enabled(can(false)),
        MenuEntry::action("Copy selected rows to the left", CopyBlockLeft).enabled(can(true)),
    ]
    .into_iter()
    .chain(in_block.then(|| {
        MenuEntry::action("Copy this difference to the right", CopyBlockHereRight)
            .enabled(can(false))
    }))
    .chain(in_block.then(|| {
        MenuEntry::action("Copy this difference to the left", CopyBlockHereLeft).enabled(can(true))
    }))
    .chain([
        MenuEntry::Separator,
        MenuEntry::action(
            if placeholder {
                "Insert a line here"
            } else {
                "Edit this line"
            },
            EditRow,
        )
        .enabled(can(side)),
        MenuEntry::action("Delete selected lines", DeleteRows).enabled(can(sel_left) && has_lines),
        MenuEntry::action("Copy text", CopyText),
        MenuEntry::Separator,
        MenuEntry::action(
            "Align this line with a line on the other side…",
            AlignWithOtherSide,
        )
        .enabled(!placeholder),
        MenuEntry::action(
            format!("Clear manual alignments ({})", view.anchors().len()),
            ClearAlignments,
        )
        .enabled(!view.anchors().is_empty()),
        MenuEntry::action("Treat lines like this as unimportant", TreatAsUnimportant)
            .enabled(!placeholder),
    ])
    .collect()
}

/// The menu of one side's path box (R12): about that side only, whichever half was pressed.
pub fn path_menu(view: &TextView, left: bool) -> Vec<MenuEntry> {
    let model = view.model.as_ref();
    let path = if left {
        &view.request.left
    } else {
        &view.request.right
    };
    let dirty = model.is_some_and(|m| m.is_dirty(left));
    let has_text = model.is_some_and(|m| {
        m.buffer(left).is_some() || (if left { m.left() } else { m.right() }).text().is_some()
    });
    let editable = model.is_some_and(|m| m.editable(left).is_ok());
    let exists = path.exists();
    macro_rules! side {
        ($label:expr, $l:expr, $r:expr) => {
            if left {
                MenuEntry::action($label, $l)
            } else {
                MenuEntry::action($label, $r)
            }
        };
    }
    vec![
        side!("Open file…", OpenLeftFile, OpenRightFile),
        side!(
            "Paste from clipboard (replace this side)",
            PasteSideLeft,
            PasteSideRight
        )
        .enabled(editable),
        MenuEntry::Separator,
        side!("Save", SaveLeft, SaveRight).enabled(dirty),
        side!("Save as…", SaveLeftAs, SaveRightAs).enabled(has_text),
        MenuEntry::Separator,
        side!("Copy path", CopyLeftPath, CopyRightPath),
        side!("Show in Explorer", RevealLeftFile, RevealRightFile).enabled(exists),
        side!(
            "Open with default app",
            OpenLeftExternally,
            OpenRightExternally
        )
        .enabled(exists),
        MenuEntry::Separator,
        MenuEntry::action("Open in hex", OpenInHex)
            .enabled(view.request.left.exists() && view.request.right.exists()),
        MenuEntry::action("Open as table", OpenAsTable)
            .enabled(view.request.left.exists() && view.request.right.exists()),
        MenuEntry::action("Open as picture", OpenAsPicture)
            .enabled(view.request.left.exists() && view.request.right.exists()),
    ]
}

/// Attaches the handlers of the path menu's actions (Open and Paste are bound in view.rs).
pub(crate) fn path_actions<E: InteractiveElement>(el: E, cx: &mut Context<TextView>) -> E {
    el.on_action(
        cx.listener(|this, _: &SaveLeft, window, cx| this.save_side(true, window, cx).detach()),
    )
    .on_action(
        cx.listener(|this, _: &SaveRight, window, cx| this.save_side(false, window, cx).detach()),
    )
    .on_action(
        cx.listener(|this, _: &SaveLeftAs, window, cx| this.save_as_dialog(true, window, cx)),
    )
    .on_action(
        cx.listener(|this, _: &SaveRightAs, window, cx| this.save_as_dialog(false, window, cx)),
    )
    .on_action(cx.listener(|this, _: &OpenInHex, _, cx| this.open_as(CompareKind::Hex, cx)))
    .on_action(cx.listener(|this, _: &OpenAsTable, _, cx| this.open_as(CompareKind::Table, cx)))
    .on_action(cx.listener(|this, _: &OpenAsPicture, _, cx| this.open_as(CompareKind::Picture, cx)))
    .on_action(cx.listener(|this, _: &CopyLeftPath, _, cx| this.copy_path(true, cx)))
    .on_action(cx.listener(|this, _: &CopyRightPath, _, cx| this.copy_path(false, cx)))
    .on_action(cx.listener(|this, _: &RevealLeftFile, _, cx| cx.reveal_path(&this.side_path(true))))
    .on_action(
        cx.listener(|this, _: &RevealRightFile, _, cx| cx.reveal_path(&this.side_path(false))),
    )
    .on_action(
        cx.listener(|this, _: &OpenLeftExternally, _, cx| {
            cx.open_with_system(&this.side_path(true))
        }),
    )
    .on_action(cx.listener(|this, _: &OpenRightExternally, _, cx| {
        cx.open_with_system(&this.side_path(false))
    }))
}

impl TextView {
    /// Asks the workspace to open this pair in another kind of tab.
    pub(crate) fn open_as(&mut self, kind: CompareKind, cx: &mut Context<Self>) {
        cx.emit(TextEvent::OpenAs { kind });
    }

    /// The message is the binary one: files that are not text, with nothing to show.
    pub(crate) fn is_binary_message(&self) -> bool {
        self.message.is_some() && self.message_hint() == Some(BINARY_HINT)
    }

    /// Both files are named like pictures, so the binary notice can offer them as such.
    pub(crate) fn both_pictures(&self) -> bool {
        use duet_core::picture::is_picture_name;
        is_picture_name(&self.request.left) && is_picture_name(&self.request.right)
    }

    fn side_path(&self, left: bool) -> PathBuf {
        if left {
            self.request.left.clone()
        } else {
            self.request.right.clone()
        }
    }

    fn copy_path(&mut self, left: bool, cx: &mut Context<Self>) {
        let path = self.side_path(left);
        cx.write_to_clipboard(ClipboardItem::new_string(path.display().to_string()));
    }

    /// Save as…: asks where, then writes the side there.
    fn save_as_dialog(&mut self, left: bool, window: &mut Window, cx: &mut Context<Self>) {
        let path = self.side_path(left);
        let dir = path.parent().map(PathBuf::from).unwrap_or_default();
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned());
        let picked = cx.prompt_for_new_path(&dir, name.as_deref());
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(path))) = picked.await else {
                return;
            };
            let _ = this.update_in(cx, |view, window, cx| {
                view.save_side_as(left, path, window, cx).detach()
            });
        })
        .detach();
    }

    pub fn menu_row_for_test(&self) -> Option<(u32, bool)> {
        self.menu_row
    }

    #[doc(hidden)]
    pub fn set_menu_row_for_test(&mut self, row: u32, left: bool) {
        self.menu_row = Some((row, left));
    }

    /// A right press on a half (or the gutter, which picks the side the selection is on, else
    /// the right one) of `display`: makes it the menu's target and selects its row unless it
    /// is already inside the selection on that side.
    pub(crate) fn press_right(
        &mut self,
        display: usize,
        left: Option<bool>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(DisplayRow::Row(row)) = self.model.as_ref().map(|m| m.display_row(display)) else {
            self.menu_row = None;
            return;
        };
        let left = left.unwrap_or_else(|| {
            self.row_sel
                .is_some_and(|s| s.left && s.rows().contains(&row))
        });
        let one = RowSel {
            left,
            anchor: row,
            head: row,
        };
        self.row_sel = Some(match self.row_sel {
            Some(s) if s.left == left && s.rows().contains(&row) => s,
            _ => one,
        });
        self.caret = None;
        self.composing = None;
        self.menu_row = Some((row, left));
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// Copy this difference: the block under the pressed row goes to the other side.
    pub(crate) fn copy_block_here(&mut self, to_left: bool, cx: &mut Context<Self>) {
        let block = self.menu_row.take().and_then(|(row, _)| {
            let model = self.model.as_ref()?;
            model.blocks().iter().position(|b| b.contains(&row))
        });
        if let Some(block) = block {
            self.copy_block(block, to_left, cx);
        }
    }

    /// F2 / Edit this line: the caret to the start of the pressed row's line (else the
    /// selection's, else the caret's own line); on a placeholder half, one empty line goes in
    /// at the gap and the caret to it (R7).
    pub(crate) fn edit_row(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(model) = self.model.as_mut() else {
            return;
        };
        // A target past the row map (the lines went away) is dropped for the selection.
        let live = |t: &(u32, bool)| model.aligned_row(t.0).is_some();
        let target = self
            .menu_row
            .take()
            .filter(live)
            .or_else(|| self.row_sel.map(|s| (s.head, s.left)).filter(live));
        let Some((row, left)) = target else {
            // Only a caret: it goes to the start of its own line, on its own side.
            let line_start = self.caret.and_then(|c| {
                let buffer = model.buffer(c.left)?;
                Some((c.left, buffer.line_start(buffer.line_of(c.head))))
            });
            match line_start {
                Some((left, offset)) => {
                    self.place_caret(Caret::at(left, offset), cx);
                    self.ensure_caret_visible(cx);
                    window.focus(&self.focus, cx);
                }
                None => self.notice = Some("Pick a line first.".into()),
            }
            cx.notify();
            return;
        };
        if let Err(reason) = model.begin_edit(left) {
            self.notice = Some(reason.into());
            cx.notify();
            return;
        }
        let placeholder = model.aligned_row(row).is_some_and(|r| {
            if left {
                r.left.is_none()
            } else {
                r.right.is_none()
            }
        });
        if placeholder {
            match model.insert_line_edit(row, left) {
                Err(reason) => {
                    self.notice = Some(reason.into());
                    cx.notify();
                }
                Ok((at, text, caret)) => {
                    if self.replace_on(left, at..at, &text, cx) {
                        self.place_caret(Caret::at(left, caret), cx);
                        self.ensure_caret_visible(cx);
                        window.focus(&self.focus, cx);
                    }
                }
            }
            return;
        }
        let line = model.side_lines(row..row + 1, left).start;
        let offset = model.line_offset(left, line as usize);
        self.place_caret(Caret::at(left, offset), cx);
        self.ensure_caret_visible(cx);
        window.focus(&self.focus, cx);
        cx.notify();
    }
}
