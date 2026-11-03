//! The right-click menu on folder rows (spec P4): what it offers for the picked rows, and the
//! handlers of the actions that only the menu has.

use std::path::PathBuf;

use duet_core::folder::entry::{Entry, EntryKind, join_rel};
use duet_core::folder::tree::Node;
use duet_core::kind::{CompareKind, detect};
use gpui_kit::component::menu::PopupMenu;
use gpui_kit::{ClipboardItem, Context, InteractiveElement, WeakEntity, Window, actions};

use crate::folder::ops::parse_masks;
use crate::folder::sync_panel::SyncSelected;
use crate::folder::view::{
    CopyToLeft, CopyToRight, DeleteBoth, DeleteLeft, DeleteRight, FolderEvent, FolderView, OpenRow,
    TouchLeft, TouchRight,
};
use crate::ui::menu::{MenuEntry, fill, fill_or_hand_back, tidy};

/// Shown while Enter or a double-click works out whether a pair opens as text or hex.
const CHECKING_KIND: &str = "Checking the file type…";

actions!(
    folder,
    [
        ToggleFolder,
        RevealLeft,
        RevealRight,
        OpenLeftExternally,
        OpenRightExternally,
        OpenAsText,
        OpenAsHex,
        OpenAsTable,
        OpenAsPicture,
        CopyPaths,
        SetAsBase,
        ExcludeItem
    ]
);

/// The menu for the picked rows. Empty without a model or a picked row (no menu opens).
pub fn row_menu(view: &FolderView) -> Vec<MenuEntry> {
    let Some(model) = view.model() else {
        return Vec::new();
    };
    let picked = model.picked();
    let nodes: Vec<&Node> = picked.iter().map(|&i| model.node(i)).collect();
    let Some(&first) = nodes.first() else {
        return Vec::new();
    };
    let idle = !view.is_busy() && !view.batch_running();
    let one = nodes.len() == 1;
    let any_left = nodes.iter().any(|n| n.left.is_some());
    let any_right = nodes.iter().any(|n| n.right.is_some());
    let any_both = nodes.iter().any(|n| n.left.is_some() && n.right.is_some());
    let on_both = first.left.is_some() && first.right.is_some();
    // A file on one side opens as text beside an untitled side (R21); the other kinds need both.
    let on_a_side = first.left.is_some() || first.right.is_some();
    let file = one && !first.is_dir;
    let folder = one && first.is_dir;

    let mut out = Vec::new();
    if file {
        out.extend([
            MenuEntry::action("Open compare", OpenRow).enabled(on_a_side),
            MenuEntry::action("Open as text", OpenAsText).enabled(on_a_side),
            MenuEntry::action("Open as hex", OpenAsHex).enabled(on_both),
            MenuEntry::action("Open as table", OpenAsTable).enabled(on_both),
            MenuEntry::action("Open as picture", OpenAsPicture).enabled(on_both),
        ]);
    }
    if folder {
        let label = if model.is_expanded(picked[0]) {
            "Collapse folder"
        } else {
            "Expand folder"
        };
        out.push(MenuEntry::action(label, ToggleFolder));
    }
    out.extend([
        MenuEntry::Separator,
        MenuEntry::action("Copy to right", CopyToRight).enabled(idle && any_left),
        MenuEntry::action("Copy to left", CopyToLeft).enabled(idle && any_right),
        MenuEntry::action("Sync selected…", SyncSelected).enabled(idle && (any_left || any_right)),
        MenuEntry::Separator,
        MenuEntry::action("Delete left (to Recycle Bin)…", DeleteLeft).enabled(idle && any_left),
        MenuEntry::action("Delete right (to Recycle Bin)…", DeleteRight).enabled(idle && any_right),
        MenuEntry::action("Delete both (to Recycle Bin)…", DeleteBoth)
            .enabled(idle && any_left && any_right),
        MenuEntry::Separator,
        MenuEntry::action("Set right modified time = left", TouchRight).enabled(idle && any_both),
        MenuEntry::action("Set left modified time = right", TouchLeft).enabled(idle && any_both),
        MenuEntry::Separator,
        MenuEntry::action("Show left in Explorer", RevealLeft).enabled(one && first.left.is_some()),
        MenuEntry::action("Show right in Explorer", RevealRight)
            .enabled(one && first.right.is_some()),
    ]);
    if file {
        out.extend([
            MenuEntry::action("Open left with default app", OpenLeftExternally)
                .enabled(first.left.is_some()),
            MenuEntry::action("Open right with default app", OpenRightExternally)
                .enabled(first.right.is_some()),
        ]);
    }
    out.extend([
        MenuEntry::action("Copy path", CopyPaths),
        MenuEntry::Separator,
    ]);
    if folder {
        out.push(
            MenuEntry::action("Set this folder as base", SetAsBase).enabled(idle && is_base(first)),
        );
    }
    out.push(MenuEntry::action("Exclude this item", ExcludeItem));
    tidy(out)
}

/// Whether both roots may move into `node`: a real folder on both sides. A file against a
/// folder would root one side on a file, and a scan walks no link.
fn is_base(node: &Node) -> bool {
    let folder = |e: &Option<Entry>| e.as_ref().is_some_and(|e| e.kind == EntryKind::Dir);
    folder(&node.left) && folder(&node.right)
}

/// A mask that excludes `rel`, rooted (`/name`) for a top-level item so a same-named item
/// deeper down stays. Glob characters in the name match themselves, and the
/// separators of the mask field (`,` `;`) match any one character, as the field cannot hold
/// them.
fn exclude_pattern(rel: &str) -> String {
    let rooted = if rel.contains('/') { "" } else { "/" };
    let escaped: String = rel
        .chars()
        .map(|c| match c {
            '*' | '?' | '[' | '{' | '}' => format!("[{c}]"),
            ',' | ';' => "?".to_owned(),
            c => c.to_string(),
        })
        .collect();
    format!("{rooted}{escaped}")
}

/// The popup of the folder list: the entries of the row last pressed with the right button.
pub(super) fn list_menu(
    view: WeakEntity<FolderView>,
) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static {
    move |menu, window, cx| {
        let Some(view) = view.upgrade() else {
            return fill(menu, Vec::new(), None);
        };
        let (entries, focus) = {
            let view = view.read(cx);
            (view.menu_entries(), view.focus.clone())
        };
        fill_or_hand_back(menu, entries, focus, window, cx)
    }
}

/// Registers the menu-only actions on the view's element.
pub(super) fn menu_actions<E: InteractiveElement>(el: E, cx: &mut Context<FolderView>) -> E {
    // Every item of the menu forgets the pressed row as it runs (capture runs before the
    // action's own handler).
    fn forget<A: gpui_kit::Action, E: InteractiveElement>(
        el: E,
        cx: &mut Context<FolderView>,
    ) -> E {
        el.capture_action(cx.listener(|this, _: &A, _, _| this.menu_row_pressed = false))
    }
    let el = forget::<OpenRow, _>(el, cx);
    let el = forget::<OpenAsText, _>(el, cx);
    let el = forget::<OpenAsHex, _>(el, cx);
    let el = forget::<OpenAsTable, _>(el, cx);
    let el = forget::<OpenAsPicture, _>(el, cx);
    let el = forget::<CopyToRight, _>(el, cx);
    let el = forget::<CopyToLeft, _>(el, cx);
    let el = forget::<SyncSelected, _>(el, cx);
    let el = forget::<DeleteLeft, _>(el, cx);
    let el = forget::<DeleteRight, _>(el, cx);
    let el = forget::<DeleteBoth, _>(el, cx);
    let el = forget::<TouchRight, _>(el, cx);
    let el = forget::<TouchLeft, _>(el, cx);
    let el = forget::<ToggleFolder, _>(el, cx);
    let el = forget::<RevealLeft, _>(el, cx);
    let el = forget::<RevealRight, _>(el, cx);
    let el = forget::<OpenLeftExternally, _>(el, cx);
    let el = forget::<OpenRightExternally, _>(el, cx);
    let el = forget::<CopyPaths, _>(el, cx);
    let el = forget::<SetAsBase, _>(el, cx);
    let el = forget::<ExcludeItem, _>(el, cx);
    el.on_action(cx.listener(|this, _: &ToggleFolder, _, cx| this.toggle_folder(cx)))
        .on_action(cx.listener(|this, _: &OpenAsText, _, cx| this.open_as(CompareKind::Text, cx)))
        .on_action(cx.listener(|this, _: &OpenAsHex, _, cx| this.open_as(CompareKind::Hex, cx)))
        .on_action(cx.listener(|this, _: &OpenAsTable, _, cx| this.open_as(CompareKind::Table, cx)))
        .on_action(
            cx.listener(|this, _: &OpenAsPicture, _, cx| this.open_as(CompareKind::Picture, cx)),
        )
        .on_action(cx.listener(|this, _: &RevealLeft, _, cx| this.reveal_side(true, cx)))
        .on_action(cx.listener(|this, _: &RevealRight, _, cx| this.reveal_side(false, cx)))
        .on_action(cx.listener(|this, _: &OpenLeftExternally, _, cx| this.open_side(true, cx)))
        .on_action(cx.listener(|this, _: &OpenRightExternally, _, cx| this.open_side(false, cx)))
        .on_action(cx.listener(|this, _: &CopyPaths, _, cx| this.copy_paths(cx)))
        .on_action(cx.listener(|this, _: &SetAsBase, _, cx| this.set_as_base(cx)))
        .on_action(cx.listener(|this, _: &ExcludeItem, window, cx| this.exclude_item(window, cx)))
}

impl FolderView {
    /// The entries the popup shows: empty unless a row was pressed since the last right press
    /// began, so blank space below the rows opens no menu.
    pub fn menu_entries(&self) -> Vec<MenuEntry> {
        if self.menu_row_pressed {
            row_menu(self)
        } else {
            Vec::new()
        }
    }

    /// Right press on a row: outside the selection it selects the row; inside, it keeps the
    /// selection and moves the cursor to the row, which the single-row handlers act on.
    pub(super) fn menu_press(&mut self, row: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(model) = &mut self.model {
            if model.is_chosen(row) {
                model.move_cursor(row);
            } else {
                model.select_row(row);
            }
        }
        self.menu_row_pressed = true;
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// The only picked node, if exactly one.
    fn only_picked(&self) -> Option<&Node> {
        let model = self.model.as_ref()?;
        match model.picked().as_slice() {
            [one] => Some(model.node(*one)),
            _ => None,
        }
    }

    fn side_path(&self, node: &Node, left: bool) -> Option<PathBuf> {
        let (entry, root) = if left {
            (node.left.as_ref(), &self.left)
        } else {
            (node.right.as_ref(), &self.right)
        };
        entry.map(|e| join_rel(root, &e.rel))
    }

    /// The picked file's pair, when it exists on both sides.
    fn picked_pair(&self) -> Option<(PathBuf, PathBuf)> {
        let node = self.only_picked().filter(|n| !n.is_dir)?;
        Some((self.side_path(node, true)?, self.side_path(node, false)?))
    }

    /// The picked file's two paths with an empty one for a missing side: what "Open as text" opens.
    fn picked_sides(&self) -> Option<(PathBuf, PathBuf)> {
        let node = self.only_picked().filter(|n| !n.is_dir)?;
        let side = |left| self.side_path(node, left).unwrap_or_default();
        Some((side(true), side(false)))
    }

    /// "Open as text" / "hex" / "table" / "picture": the kind is the user's choice, so nothing is sniffed.
    /// Only text opens a file that exists on one side.
    fn open_as(&mut self, kind: CompareKind, cx: &mut Context<Self>) {
        let pair = if kind == CompareKind::Text {
            self.picked_sides()
        } else {
            self.picked_pair()
        };
        if let Some((left, right)) = pair {
            // A newer open: a kind check still running for an earlier one is dropped.
            self.drop_detect(cx);
            cx.emit(FolderEvent::OpenFiles {
                left,
                right,
                kind: Some(kind),
            });
        }
    }

    /// The selected row's file, when it is on both sides: what Enter opens.
    pub(super) fn selected_pair(&self) -> Option<(PathBuf, PathBuf)> {
        let model = self.model.as_ref()?;
        let node = model.node_at(model.selected_row()?);
        match (&node.left, &node.right) {
            (Some(l), Some(r)) if !node.is_dir => {
                Some((join_rel(&self.left, &l.rel), join_rel(&self.right, &r.rel)))
            }
            _ => None,
        }
    }

    /// The selected file's paths when it is on one side only, an empty path for the other (R21).
    pub(super) fn selected_sides(&self) -> Option<(PathBuf, PathBuf)> {
        let model = self.model.as_ref()?;
        let node = model.node_at(model.selected_row()?);
        if node.is_dir || (node.left.is_none() && node.right.is_none()) {
            return None;
        }
        let side = |left| self.side_path(node, left).unwrap_or_default();
        Some((side(true), side(false)))
    }

    /// Whether the selected row is a folder on both sides (so no link and no file on either:
    /// R22).
    pub(super) fn selected_folder_is_enterable(&self) -> bool {
        let Some(model) = self.model.as_ref() else {
            return false;
        };
        model
            .selected_row()
            .is_some_and(|row| is_base(model.node_at(row)))
    }

    /// Enter / double-click: `detect` reads the start of both files, which on a network share
    /// can take seconds, so it runs in the background under a notice and the tab opens when it
    /// answers. A pair it cannot read opens as text, which says why. The answer is dropped
    /// when something newer came first: another open, F5, F9 or new roots (each cancels the
    /// check), or another row selected by then.
    pub(super) fn open_detected(&mut self, left: PathBuf, right: PathBuf, cx: &mut Context<Self>) {
        let (generation, _) = self.detect.start();
        self.notice = Some(CHECKING_KIND.into());
        cx.notify();
        let background = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            let (l, r) = (left.clone(), right.clone());
            let kind = background.spawn(async move { detect(&l, &r) }).await;
            let kind = kind.ok().filter(|kind| *kind != CompareKind::Folder);
            let _ = this.update(cx, |view, cx| {
                view.detected(generation, left, right, kind, cx)
            });
        })
        .detach();
    }

    /// The kind check `generation` answered: open the pair unless something newer came first.
    fn detected(
        &mut self,
        generation: u64,
        left: PathBuf,
        right: PathBuf,
        kind: Option<CompareKind>,
        cx: &mut Context<Self>,
    ) {
        if !self.detect.is_current(generation) {
            return;
        }
        self.drop_detect(cx);
        if self.selected_pair() == Some((left.clone(), right.clone())) {
            cx.emit(FolderEvent::OpenFiles { left, right, kind });
        }
    }

    /// Forgets the running kind check, if any, and its notice.
    pub(super) fn drop_detect(&mut self, cx: &mut Context<Self>) {
        self.detect.cancel();
        if self.notice.as_deref() == Some(CHECKING_KIND) {
            self.notice = None;
            cx.notify();
        }
    }

    fn toggle_folder(&mut self, cx: &mut Context<Self>) {
        if let Some(model) = &mut self.model
            && let Some(row) = model.selected_row()
        {
            model.toggle_row(row);
            cx.notify();
        }
    }

    fn reveal_side(&mut self, left: bool, cx: &mut Context<Self>) {
        let path = self.only_picked().and_then(|n| self.side_path(n, left));
        if let Some(path) = path {
            cx.reveal_path(&path);
        }
    }

    fn open_side(&mut self, left: bool, cx: &mut Context<Self>) {
        let path = self
            .only_picked()
            .filter(|n| !n.is_dir)
            .and_then(|n| self.side_path(n, left));
        if let Some(path) = path {
            cx.open_with_system(&path);
        }
    }

    /// One line per picked item: its left path, else its right path.
    fn copy_paths(&mut self, cx: &mut Context<Self>) {
        let Some(model) = &self.model else {
            return;
        };
        let lines: Vec<String> = model
            .picked()
            .into_iter()
            .filter_map(|i| {
                let node = model.node(i);
                self.side_path(node, true)
                    .or_else(|| self.side_path(node, false))
            })
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        if !lines.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(lines.join("\r\n")));
        }
    }

    /// Both roots move into the picked folder, when it is a folder on both sides.
    pub(super) fn set_as_base(&mut self, cx: &mut Context<Self>) {
        if self.is_busy() || self.batch_running() {
            return;
        }
        let roots = self
            .only_picked()
            .filter(|n| is_base(n))
            .and_then(|n| Some((self.side_path(n, true)?, self.side_path(n, false)?)));
        if let Some((left, right)) = roots {
            self.set_roots(left, right, cx);
        }
    }

    /// Adds the picked items to the exclude masks and compares again.
    fn exclude_item(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(model) = &self.model else {
            return;
        };
        let new: Vec<String> = model
            .picked()
            .into_iter()
            .map(|i| exclude_pattern(&model.node(i).rel))
            .collect();
        let input = self.exclude_input().clone();
        let mut masks = parse_masks(&input.read(cx).value());
        for pattern in new {
            if !masks.contains(&pattern) {
                masks.push(pattern);
            }
        }
        let text = masks.join(", ");
        input.update(cx, |state, cx| state.set_value(text, window, cx));
        self.apply_masks(cx);
    }
}

#[cfg(test)]
mod tests {
    use super::exclude_pattern;
    use duet_core::folder::entry::Filters;
    use duet_core::folder::scan::validate_filters;

    #[test]
    fn exclude_pattern_keeps_plain_paths_and_escapes_glob_characters() {
        assert_eq!(exclude_pattern("sub/d"), "sub/d");
        assert_eq!(exclude_pattern("d"), "/d");
        assert_eq!(exclude_pattern("a[1]*.txt"), "/a[[]1][*].txt");
        assert_eq!(exclude_pattern("a,b;c"), "/a?b?c");
        let filters = Filters {
            include: Vec::new(),
            exclude: vec![exclude_pattern("{x}[1]?")],
        };
        validate_filters(&filters).unwrap();
    }
}
