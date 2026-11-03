//! The one icon table: each action duet's chrome offers has one Lucide icon, the same in every
//! view. Controls take their icons from here, and `app::assets` embeds exactly these, since an
//! icon that is not embedded draws as nothing.

use gpui_kit::assets::IconName;

/// Each constant, and `ALL` listing them for the embed test.
macro_rules! icon_table {
    ($($(#[$doc:meta])* $name:ident = $icon:ident;)*) => {
        $($(#[$doc])* pub const $name: IconName = IconName::$icon;)*

        /// Every icon above, which `app::assets` must embed.
        pub const ALL: &[IconName] = &[$($name),*];
    };
}

icon_table! {
    /// Swap the two sides.
    SWAP = ArrowLeftRight;
    /// Reload, or compare again.
    RELOAD = RefreshCw;
    /// The first difference.
    FIRST = ArrowUpToLine;
    /// The previous difference, match, conflict or group.
    PREV = ChevronUp;
    /// The next difference, match, conflict or group.
    NEXT = ChevronDown;
    /// The last difference.
    LAST = ArrowDownToLine;
    UNDO = Undo2;
    REDO = Redo2;
    FIND = Search;
    GO_TO_LINE = Hash;
    COPY_PATH = Copy;
    /// Paste the clipboard as a table.
    PASTE_TABLE = ClipboardPaste;
    /// Close a tab, a find or go-to bar, a merge slot, the result preview or a recent entry.
    CLOSE = X;
    NEW_TAB = Plus;
    ZOOM_OUT = ZoomOut;
    ZOOM_IN = ZoomIn;
    /// Reset a merge part to its default.
    RESET = RotateCcw;
    /// Up one folder on both sides.
    UP = ArrowUp;
    /// A list with chevrons, not bare chevrons: at 15 px those read as an × beside the tree.
    EXPAND_ALL = ListChevronsUpDown;
    COLLAPSE_ALL = ListChevronsDownUp;
    LEGEND = Info;
    SYNC = FolderSync;
    /// Copy to the right: everything, a block or the selected rows.
    TO_RIGHT = ArrowRight;
    /// Copy to the left: everything, a block or the selected rows.
    TO_LEFT = ArrowLeft;
    /// A dropdown's caret.
    CARET = ChevronDown;
    /// A closed folder's caret in the tree.
    TREE_CLOSED = ChevronRight;
    /// An open folder's caret in the tree.
    TREE_OPEN = ChevronDown;
    HELP = CircleQuestionMark;
    /// The theme button shows the theme a click gives: the moon while light...
    TO_DARK = Moon;
    /// ...and the sun while dark.
    TO_LIGHT = Sun;
    /// A toggle's check mark.
    CHECK = Check;
    /// Back to the folder compare.
    BACK = ArrowLeft;
    /// Run the three-way merge.
    RUN = ChevronRight;
    /// A kind of compare, on the Start tab's tiles and recent entries: text files...
    KIND_FILES = Files;
    /// ...folders...
    KIND_FOLDERS = FolderTree;
    /// ...a 3-way merge...
    KIND_MERGE = GitMerge;
    /// ...binary files in hex...
    KIND_HEX = Binary;
    /// ...tables...
    KIND_TABLE = Sheet;
    /// ...and pictures.
    KIND_PICTURE = Image;
    /// The Start form's drop hint.
    DROP = Upload;
    /// "Paste text…": an untitled pair to type or paste into.
    PASTE_TEXT = ClipboardType;
    /// The empty recent list.
    NO_RECENT = Clock;
}
