//! The help in English. Every control is named as duet shows it, as a `[chip]`, and every key
//! as `{Keys}` (`markup`); quotes are left around literal text only. Each topic says what its
//! Thai twin in `th.rs` says, block for block, naming the same controls and keys.

use super::topics::{Block, PageCopy, TopicText, WelcomeCopy};

pub const PAGE: PageCopy = PageCopy {
    search: "Search: EOL, sync, report",
    search_name: "Search the help",
    empty: "No topic matches.",
    prev: "Previous",
    next: "Next",
};

pub const WELCOME: WelcomeCopy = WelcomeCopy {
    title: "Welcome to duet",
    lines: [
        "Drop two files or folders to compare them at once",
        "or pick a compare type below",
        "Press {F1} for the guide (Thai and English)",
    ],
    open: "Open the guide",
    dismiss: "Don't show again",
};

pub const START: TopicText = TopicText {
    title: "Getting started",
    keywords: "start open file folder drag drop path command line recent tab new compare offline",
    body: &[
        Block::Para(
            "duet compares files and folders on this computer. Nothing is sent anywhere: every file is read and compared here.",
        ),
        Block::Heading("The Start tab"),
        Block::Bullets(&[
            "Type or paste two paths into the [Left] and [Right] boxes, then press [Compare] or {Enter}.",
            "duet picks the view itself: two folders open a folder compare, two pictures a picture compare, binary files the hex view, and anything else the text compare.",
            "[File…] and [Folder…] beside each box choose a path.",
        ]),
        Block::Heading("Dropping, tiles and text"),
        Block::Bullets(&[
            "The form is also a drop zone: drop files or folders on it, or anywhere on the window. Two items fill both boxes and compare at once; one item fills the box you drop it on. While you drag, the form's border lights up.",
            "The six tiles under the form each start one kind of compare: [Compare files], [Compare folders], [3-way merge], [Compare binary files], [Compare tables] and [Compare pictures].",
            "The bar at the top of the window, from [Start] to [Table], goes to the tab of that kind you used last. With none open, it starts a new compare, as the tile does.",
            "[Paste text…], beside [Compare], opens two empty sides to type or paste into; you can open files into either side later.",
        ]),
        Block::Heading("Recent"),
        Block::Bullets(&[
            "Every compared pair goes to the [Recent] list. Click an entry to open it again.",
            "Right-click an entry for [Open] or [Remove from list]. [Remove from the list] on the entry removes it too, and [Clear] empties the list.",
            "An entry whose files are gone is dimmed.",
        ]),
        Block::Heading("Tabs"),
        Block::Bullets(&[
            "Each compare opens in its own tab. Opening a pair that is already open goes back to its tab.",
            "[New compare] or {Ctrl+T} opens a new Start tab. {Ctrl+W}, [Close tab] on the tab or a middle click closes it. {Ctrl+Tab} goes to the next tab.",
            "[Switch between light and dark], at the top right, changes the theme.",
        ]),
        Block::Heading("Command line"),
        Block::Bullets(&[
            "duet LEFT RIGHT compares two files or two folders.",
            "duet --table LEFT RIGHT opens two files as tables.",
            "duet LEFT RIGHT BASE [[OUTPUT]] starts a 3-way merge, and saves the result to OUTPUT when you give one.",
            "duet --select-left PATH remembers PATH as the left side and exits without opening a window.",
            "duet --compare-to PATH compares PATH with the remembered left side.",
            "duet --register-shell adds those two commands to Explorer's right-click menu for this duet.exe.",
            "duet --unregister-shell removes them again.",
            "To use duet from git difftool and mergetool, see the git recipe in the README.",
        ]),
    ],
};

pub const FOLDER: TopicText = TopicText {
    title: "Comparing folders",
    keywords: "folder criterion content quick size time filter include exclude mask count select tree flat chip case",
    body: &[
        Block::Para(
            "Open two folders from the Start tab, or choose them with [Left] and [Right] in a folder tab. [Compare] ({F5}) compares again, [Swap sides] ({F9}) swaps the sides and [Up one folder on both sides] goes up one level.",
        ),
        Block::Heading("How files are compared"),
        Block::Para("The criterion button chooses how two files are compared:"),
        Block::Bullets(&[
            "[Quick (size + time)]: the fastest, since it looks only at the size and the modified time and reads no files; for a first look at a big folder. Set the tolerance in seconds, and [Ignore 1 h DST shift].",
            "[Content]: reads the files and compares their bytes. The modified time does not count.",
            "[Size only]: only the size counts, so two files of the same size look the same.",
            "[Content by the text compare rules]: compares the files with the rules of the text compare, so a file that differs only in parts you ignore shows ≈.",
        ]),
        Block::Para("duet starts with Quick and remembers the last choice."),
        Block::Heading("What you see"),
        Block::Bullets(&[
            "The chips [All], [Mismatched], [Changed], [Left only], [Right only], [Same] and [Moved] choose which rows show. [Mismatched] is the changed files plus the one-sided ones.",
            "\"3 files\" on a folder counts the files below it that the chip and the path filter keep: files, not lines. Hover it for the whole folder's breakdown.",
            "[View: tree] switches to a flat list of files that you can sort by any column.",
            "A left-only and a right-only file with the same bytes are tagged as moved or renamed; click the tag to go to the other file. Only [Content] and [Content by the text compare rules] look for them, and [Find moved files] in [Options] turns it off.",
            "[Legend] explains every symbol; see \"Symbols and colours\".",
        ]),
        Block::Heading("Selecting and opening"),
        Block::Bullets(&[
            "Click a row. {Shift}+click selects a range, {Ctrl}+click adds or removes one row, a drag selects rows and {Ctrl+A} selects all.",
            "{↑} and {↓} move. {→} and {←} open and close a folder. {Enter} or a double-click opens a file pair in its own tab.",
            "{F8} and {F7} go to the next and previous difference.",
            "Right-click a file for [Open as text], [Open as hex], [Open as table] or [Open as picture].",
        ]),
        Block::Heading("Filters"),
        Block::Bullets(&[
            "[Include] and [Exclude] take masks separated by ; or , such as *.xml; *.properties or target; *.log. Press {Enter} to apply them.",
            "A mask that starts with / matches the top level only: /web.config leaves app/web.config alone.",
            "Right-click a row and choose [Exclude this item] to add it to [Exclude].",
            "[Filter paths] ({Ctrl+F}) keeps the rows whose path holds the text, in any case.",
        ]),
        Block::Para(
            "Names that differ only in case, such as Config.xml and config.xml, are one file on Windows. duet pairs them and marks them ⚠, because on Linux they are two files.",
        ),
    ],
};

pub const LEGEND_TOPIC: TopicText = TopicText {
    title: "Symbols and colours",
    keywords: "legend symbol colour color glyph newer hatched moved purple unimportant anchor",
    body: &[
        Block::Heading("The folder compare"),
        Block::Legend,
        Block::Heading("The text compare"),
        Block::Bullets(&[
            "Amber: a changed line; the characters that differ are marked.",
            "Red: a line on the left only. The other side is hatched.",
            "Green: a line on the right only.",
            "Blue with ↷ 7 or ↶ 2: a line that moved; the number is its line on the other side. Click it to go there, or drag it to move the line.",
            "Purple: a difference the rules count as unimportant. It is not counted.",
            "⍽ and ◇: invisible characters; see \"Invisible characters\".",
            "⚓: lines you aligned with each other yourself.",
        ]),
        Block::Para("On a wide window the status bar names the five colours."),
    ],
};

pub const EOL: TopicText = TopicText {
    title: "Line endings and BOM only",
    keywords: "eol crlf lf bom final newline line ending script linux encoding",
    body: &[
        Block::Para("Some files match line for line, yet their bytes differ. The usual causes:"),
        Block::Bullets(&[
            "Line endings: Windows ends a line with CRLF (2 bytes), Linux with LF (1 byte).",
            "The final newline: the last line ends with a line break on one side only.",
            "BOM: 3 bytes that some programs put at the start of a UTF-8 file.",
        ]),
        Block::Para(
            "The folder compare shows such files as ≈ and counts them with Same. Hover the row to see which side has which format.",
        ),
        Block::Heading("The exception: scripts ⚠"),
        Block::Para(
            "A Linux script (for example .sh or .py, or a file that starts with #!) with CRLF or a BOM counts as different and shows ⚠, because it may not run: \"/bin/bash^M: bad interpreter\".",
        ),
        Block::Para(
            "To fix one, open it in a text tab, choose [On save: keep] › [Line endings: LF] (for a BOM, [Encoding: UTF-8]) and save.",
        ),
        Block::Heading("Your choice"),
        Block::Bullets(&[
            "Turn on [Count line-ending and BOM differences] beside the criterion to count every such file as different. It works with [Content] and the text rules.",
            "Files whose encoding differs, for example UTF-8 and windows-874, always count as different, because a program may read the Thai wrongly.",
            "In a text tab, [Line endings] under Ignore hides line-ending differences, and the ⚠ line in the status bar names what differs.",
        ]),
    ],
};

pub const ODD: TopicText = TopicText {
    title: "Invisible characters",
    keywords: "invisible nbsp lrm rlm zero-width zwsp zwj bom space unicode hidden character",
    body: &[
        Block::Para(
            "Some characters look like a space, or like nothing at all, yet a computer reads them as other characters. They often come along when text is copied from Word, e-mail, a chat or a web page.",
        ),
        Block::Bullets(&[
            "NBSP (U+00A0): a no-break space that looks exactly like a space.",
            "LRM and RLM (U+200E, U+200F): zero-width direction marks, often copied with dates or certificate thumbprints.",
            "ZWSP and ZWJ (U+200B, U+200D): zero-width spaces that can hide inside a word.",
            "A BOM in the middle of a file (U+FEFF), often left when files are joined.",
        ]),
        Block::Para(
            "In a value, such as a timeout, a host name or a thumbprint, the text looks right but does not match, so a program may fail to read it.",
        ),
        Block::Heading("How duet shows them"),
        Block::Bullets(&[
            "The text compare marks a space-like character with ⍽ and a zero-width, direction or control character with ◇.",
            "A button in the status bar counts them: in the warning colour when they are in the text, in a quieter colour when they are only in comments or indentation. Click it for the next one, {Shift}+click for the previous.",
            "After a folder compare duet looks through the files and marks them \"⚠ invisible\", or \"invisible\" when they are only in comments or indentation. [Options] turns this off; above 10,000 files, press [Look for invisible characters].",
            "Files that differ only in the kind of whitespace show ⚠ and count as different, because the values differ.",
        ]),
        Block::Heading("How to remove them"),
        Block::Para(
            "Open the file in a text tab and press {Ctrl+F}. Turn on [.*] (regular expression), search \\u00A0, type one space in [Replace with] and press [Replace all]. For the zero-width ones search [[\\u200B-\\u200F\\uFEFF]] and replace them with nothing. Then press {Ctrl+S}; {Ctrl+Z} undoes it before you save.",
        ),
    ],
};

pub const OPS: TopicText = TopicText {
    title: "Copy, delete and sync",
    keywords: "copy delete sync mirror update recycle bin touch time base",
    body: &[
        Block::Bullets(&[
            "Copy: select rows, then press [Copy] ({Alt+→}) or [Copy] ({Alt+←}): the arrow on each points where the files go. Or right-click › [Copy to right] or [Copy to left]. A copy keeps the file's modified time.",
            "duet asks before it copies, and always before it replaces a file. Turn off [Options] › [Ask before copying] to skip the question for copies that replace nothing.",
            "Delete: [Delete…] ({Del}), or right-click › [Delete left (to Recycle Bin)…]. Files go to the Recycle Bin, so you can restore them.",
            "Times: right-click › [Set right modified time = left] changes the time only, never the contents.",
            "Right-click a folder › [Set this folder as base] to compare inside it on both sides; [Up one folder on both sides] goes back up.",
        ]),
        Block::Heading("Sync"),
        Block::Para(
            "[Sync…] opens a panel that lists every step before anything happens. Choose a mode, read the preview, then press [Start sync].",
        ),
        Block::Bullets(&[
            "[Update right] or [Update left]: copy new files, and files that are newer on the other side. A file that is newer where it would land is skipped.",
            "[Update both]: the newer side wins, and missing files are copied both ways.",
            "[Mirror to right] or [Mirror to left]: make the other side the same, which moves its extra files to the Recycle Bin.",
            "The updates skip files whose times are equal or nearly equal; the preview says why.",
            "[Selected items only] syncs the selected rows; right-click › [Sync selected…] starts with it.",
            "{Esc} stops a copy, a delete or a sync. Nothing goes to the Recycle Bin after a cancelled copy.",
            "When one folder is inside the other, sync is off, so it cannot overwrite its own source.",
        ]),
        Block::Para(
            "Tip: before a sync between environments, select only the files you need and choose [Selected items only]. It is safer than syncing the whole folder.",
        ),
    ],
};

pub const REPORT: TopicText = TopicText {
    title: "Folder reports",
    keywords: "report html excel xlsx csv txt export print pdf password mask share",
    body: &[
        Block::Para("In a folder tab, [Report] offers four formats:"),
        Block::Bullets(&[
            "HTML report: a summary and each file's details, for a browser. It needs no network; {Ctrl+P} in the browser prints it or saves a PDF.",
            "Excel (.xlsx): a workbook to track the review as a team, with a \"Result\" column to fill in.",
            "Text report (.txt) and CSV: plain text, and a table for other programs.",
        ]),
        Block::Heading("The choices"),
        Block::Bullets(&[
            "Names for A and B, for example IWT and PRD: a reader does not know what \"left\" means. duet remembers them for the folder pair.",
            "A title and a note, for example the change number.",
            "The report's language, Thai or English: it starts as the program's and need not match it.",
            "The scope: [All items], [As filtered now] or [Selected items only].",
            "[Show each file's differing lines], with the number of context lines. It needs [Content] or the text rules as the criterion.",
            "[Hide values that look like passwords…], recommended before sharing: a value such as password=… is shown as •••••• with a short code, the same code for the same value. It guesses from names, so read the report before it leaves the team.",
        ]),
        Block::Para(
            "Press [Create] and choose where to save; duet then reads the files and writes the report. {Esc} cancels it before anything is written.",
        ),
    ],
};

pub const TEXT: TopicText = TopicText {
    title: "Comparing and editing text",
    keywords: "text edit save copy block find replace go to line filter encoding wrap details report patch untitled reload",
    body: &[
        Block::Heading("Moving around"),
        Block::Bullets(&[
            "{F8} and {F7} go to the next and previous difference, {Ctrl+Home} and {Ctrl+End} to the first and last. Without a caret, {N} and {P} do the same.",
            "The strip at the right edge is a map of the differences: click or drag it.",
            "{Ctrl+1} to {Ctrl+4} show all lines, the differences, the differences with context, or the same lines.",
            "{Ctrl+F} finds and replaces, with [Aa] (case), [Word] and [.*] (regular expression). {Ctrl+G} goes to a line.",
        ]),
        Block::Heading("Editing"),
        Block::Bullets(&[
            "Copy a block with the arrows in the middle, {Alt+→} and {Alt+←}, or {Ctrl+R} and {Ctrl+L}. The two [All] buttons copy everything to the right or to the left.",
            "Click into a line and type. {Ctrl+Z} undoes, {Ctrl+Y} redoes, and {Ctrl+S} saves the edited side ([Save left], [Save right]). {Ctrl+Shift+S} saves the side under a new name.",
            "[On save: keep] chooses the line endings and the encoding a save writes; otherwise a save keeps the file's own.",
            "When another program changes an open file, a bar offers [Reload] and [Ignore]. {F5} reloads too.",
        ]),
        Block::Heading("Display"),
        Block::Bullets(&[
            "[Wrap] wraps long lines, [Details] shows the whole selected line under the rows, and [Font 12.5] and [Tab 4] set the text size and the tab width.",
            "Thai text looks wrong? Click the encoding in that side's file-name bar (such as UTF-8 · LF) and choose the right one, for example [Thai (windows-874)]. duet usually detects it.",
            "Binary files show a message; [Open in hex] or [Open as text] opens them anyway.",
            "[Sort keys], beside [Format], shows JSON or YAML with its keys sorted and indented the same, so files that differ only in key order match. Line numbers are those of the sorted text.",
            "A value written differently, like 1.0 and 1, still shows as different, and the sorted view is read-only.",
        ]),
        Block::Heading("More"),
        Block::Bullets(&[
            "[Report]: an HTML report side by side, a patch file (unified diff), a text report, or the patch copied to the clipboard.",
            "[Paste text…] on the Start tab opens two empty sides to type or paste into; {Ctrl+S} asks where to save each one.",
        ]),
    ],
};

pub const RULES: TopicText = TopicText {
    title: "Ignoring unimportant differences",
    keywords: "rules ignore whitespace case blank comments regex date guid version unimportant purple",
    body: &[
        Block::Para(
            "Use these when only the differences that matter should show, for example logs whose times differ, or files indented another way.",
        ),
        Block::Bullets(&[
            "[Whitespace: compare]: compare it, or ignore it at the end of lines, at the start, at both edges, its amount, or all of it.",
            "Under Ignore: [Case], [Line endings], [Blank lines] and [Comments]. Comments follow the file's type; [Type: Auto] lets you choose another.",
            "[Rules…]: regular expressions for text that does not matter, such as dates, GUIDs or version numbers (the + buttons add them), and for whole lines that do not matter. Press [Apply].",
            "Right-click a line › [Treat lines like this as unimportant] makes a line rule from it.",
        ]),
        Block::Para(
            "Unimportant parts show in purple and are not counted as differences. A folder compare uses the same rules with [Content by the text compare rules].",
        ),
        Block::Para(
            "For a file over 64 MiB the blank-line, comment and pattern rules are skipped; the status bar says so.",
        ),
    ],
};

pub const MERGE: TopicText = TopicText {
    title: "3-way merge",
    keywords: "merge 3-way three conflict base result markers",
    body: &[
        Block::Para(
            "Use it when two copies were edited from the same original, the base: for example a config that two teams changed.",
        ),
        Block::Bullets(&[
            "Click the [3-way merge] card, then [Open…] on the left, base and right slots, or drop three files. [Paste text] lets you paste the texts instead. Then press [Merge].",
            "From the command line: duet LEFT RIGHT BASE [[OUTPUT]].",
            "A change on one side is taken by itself. Where both sides changed the same lines you get a CONFLICT: choose [Use left], [Use right], [Left + right], [Right + left] or [Use base], or type the result.",
            "{F8} and {F7} go to the next and previous conflict. {Alt+←} and {Alt+→} use the left or the right; {Ctrl+Z} undoes a choice.",
            "[Resolve all with left] or [Resolve all with right] settles every open conflict at once.",
            "{Ctrl+S} or [Save result] saves the result; [Save result to…] asks where. With conflicts left, duet asks first and can save them with conflict markers.",
            "Line endings and a missing final newline never make a conflict.",
        ]),
    ],
};

pub const HEX: TopicText = TopicText {
    title: "Hex",
    keywords: "hex binary byte offset bytes find copy",
    body: &[
        Block::Para(
            "For binary files of any size: both files side by side, byte by byte. Open them with the [Compare binary files] card, or [Open as hex] in a folder compare.",
        ),
        Block::Bullets(&[
            "Bytes that differ are marked. {F8} and {F7} go to the next and previous difference, {Ctrl+Home} and {Ctrl+End} to the first and last.",
            "[Differences only] ({Ctrl+2}) hides the rows that match; [All] ({Ctrl+1}) shows them again.",
            "[Bytes per row: 16] chooses 8, 16 or 32.",
            "Type an offset, for example 0x1F40, and press [Go to]. {Ctrl+G} jumps to the offset box.",
            "[Find…] ({Ctrl+F}) looks for hex bytes or text on the left or the right; {F3} finds the next.",
            "[Copy hex] ({Ctrl+C}) copies the selected bytes.",
        ]),
    ],
};

pub const PICTURE: TopicText = TopicText {
    title: "Pictures",
    keywords: "picture image png jpeg gif bmp webp ico tiff svg pixel tolerance zoom slider blink",
    body: &[
        Block::Para(
            "PNG, JPEG, GIF, BMP, WebP, ICO, TIFF and SVG, compared pixel by pixel. A GIF shows its first frame only.",
        ),
        Block::Bullets(&[
            "Four modes: [Side by side] ({1}), [Difference] ({2}), which shows the differing pixels in red, [Slider] ({3}) and [Blink] ({4}).",
            "[Tolerance] (0 to 255) lets small colour differences pass, for example those JPEG compression makes.",
            "Zoom: [Fit] ({Ctrl+0}), [1:1] ({Ctrl+1}), [Zoom in] and [Zoom out], or {Ctrl}+wheel. Both sides zoom and pan together.",
            "{F8} and {F7} go to the next and previous group of differing pixels.",
            "Pictures of different sizes: only the overlap is compared, and the status bar says so.",
            "[Export difference image] saves the difference as a PNG.",
        ]),
    ],
};

pub const TABLE: TopicText = TopicText {
    title: "Tables (CSV, TSV, Excel)",
    keywords: "table csv tsv excel xlsx xlsm workbook sheet formula key column delimiter header cell number",
    body: &[
        Block::Para(
            "duet compares CSV and TSV files cell by cell, and Excel workbooks (.xlsx and .xlsm), read-only: it never writes a workbook. Each side shows one sheet, chosen with the sheet picker in the toolbar ({Ctrl+PgUp} and {Ctrl+PgDn} for the left side, with {Shift} for the right side). Both sides start on the first sheet whose name they share. .xls and .xlsb files are not read; you can still copy cells in Excel and paste them with the paste button beside [Open left…] or [Open right…].",
        ),
        Block::Para(
            "A cell shows its stored value, not Excel's format: a cell that Excel shows as 25% shows 0.25 here, and dates show as 2024-03-05. Numbers read at 15 significant digits, Excel's own precision, so a result stored as 0.30000000000000004 reads 0.3. A formula compares by the result saved in the file. When both sides are workbooks, a text cell that looks like a number or a date shows with a leading ', like Excel's own notation, so the text 1 differs from the number 1. Beside a CSV file the ' is left off, because a CSV has no cell types. [Compare numbers by value] makes '1 equal 1, in key columns too; a text TRUE or a text date stays text.",
        ),
        Block::Para(
            "A folder compare compares the bytes of two workbooks, and Excel rewrites the file on every save: open the pair as a table to see whether the cells differ. [Export] writes CSV or HTML; saving back to .xlsx is not offered.",
        ),
        Block::Para(
            "A password-protected workbook is refused: remove the password in Excel and save a copy. A very large sheet is cut where duet stops reading, and a notice says so.",
        ),
        Block::Para(
            "Open tables with the [Compare tables] card, [Open as table] in a folder compare, or duet --table LEFT RIGHT.",
        ),
        Block::Bullets(&[
            "[Delimiter: Auto] detects a comma, tab, semicolon or pipe; choose one to force it. It applies to CSV sides only.",
            "[First row is a header]: columns pair by name. Turn it off to pair them by position.",
            "[Key columns]: type a column name or number, for example id or 1,3, to pair rows by key even when their order differs. Leave it empty to pair rows by order.",
            "[Ignore case], [Trim spaces] and [Compare numbers by value], so 1.0 equals 1.",
            "The chips [All], [Differences], [Same], [Left only] and [Right only] filter the rows; {Ctrl+1}, {Ctrl+2} and {Ctrl+3} choose All, Differences and Same.",
            "Select a cell to see both values in full under the table, with the changed characters marked.",
            "{F8} and {F7} go to the next and previous difference, {Ctrl+F} finds a value and {Ctrl+G} goes to a line.",
            "[Export]: the differences as CSV, or an HTML report.",
        ]),
    ],
};

pub const KEYS: TopicText = TopicText {
    title: "Keyboard shortcuts",
    keywords: "shortcut keyboard key hotkey f1",
    body: &[
        Block::Para(
            "{F1} opens this help from any tab. The keys below work in the compares where they apply.",
        ),
        Block::Shortcuts,
    ],
};

pub const FAQ: TopicText = TopicText {
    title: "Questions",
    keywords: "faq question why slow settings data safety offline other time",
    body: &[
        Block::Heading("The contents match, yet the files show as different?"),
        Block::Para(
            "Look at the criterion: [Quick (size + time)] does not read the files, so the same contents with another time count as different. Choose [Content]. A file that shows ≈ differs only in its line endings or BOM; see \"Line endings and BOM only\".",
        ),
        Block::Heading("Why does a same file say \"other time\"?"),
        Block::Para(
            "Its contents match byte for byte, but the modified times differ. That is not counted as a difference.",
        ),
        Block::Heading("Two lines look the same but are marked as different?"),
        Block::Para(
            "An invisible character may be in one of them. Look for ⍽ or ◇ in the line (see \"Invisible characters\"), or turn on [Show spaces] to tell tabs from spaces.",
        ),
        Block::Heading("A big folder is slow?"),
        Block::Para(
            "Leave out what you do not need with [Include] and [Exclude], or compare with [Quick (size + time)] first. {Esc} stops a running compare.",
        ),
        Block::Heading("Thai text shows as strange characters?"),
        Block::Para(
            "In the text tab, click the encoding in that side's file-name bar and choose the right one, for example [Thai (windows-874)].",
        ),
        Block::Heading("Is anything sent outside this computer?"),
        Block::Para("No. duet reads and compares the files on this computer only."),
        Block::Heading("Where are the settings kept?"),
        Block::Para(
            "In %LOCALAPPDATA%\\duet: settings.json, the recent list (recent.txt) and the theme (theme.txt).",
        ),
    ],
};
