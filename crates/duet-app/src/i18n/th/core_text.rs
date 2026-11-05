//! Core's labels and sentences the UI draws, and the error sentences `i18n/errors.rs` says. The
//! English is core's own text, marked `tr_key` there, so the table finds it by that text.

pub const ENTRIES: &[(&str, &str)] = &[
    // ---- the merge choices, their tips and the walk ----
    ("Use left", "ใช้ซ้าย"),
    ("Use right", "ใช้ขวา"),
    ("Left + right", "ซ้าย + ขวา"),
    ("Right + left", "ขวา + ซ้าย"),
    ("Use base", "ใช้ฐาน"),
    ("Edited", "แก้เอง"),
    ("Use the left text (Alt+←)", "ใช้ข้อความฝั่งซ้าย (Alt+←)"),
    ("Use the right text (Alt+→)", "ใช้ข้อความฝั่งขวา (Alt+→)"),
    (
        "Use both: the left text, then the right",
        "ใช้ทั้งคู่: ข้อความซ้าย ตามด้วยขวา",
    ),
    (
        "Use both: the right text, then the left",
        "ใช้ทั้งคู่: ข้อความขวา ตามด้วยซ้าย",
    ),
    (
        "Keep the base text and drop both sides' changes",
        "คงข้อความฐานไว้ และทิ้งการแก้ไขของทั้งสองฝั่ง",
    ),
    ("Change", "จุดต่าง"),
    ("Conflict", "conflict"),
    ("{} {} / {}", "{} {} / {}"),
    // ---- why a workbook could not be read ----
    ("the rows are out of order", "แถวเรียงผิดลำดับ"),
    ("the cells of a row are out of order", "เซลล์ในแถวเรียงผิดลำดับ"),
    ("there is no such sheet", "ไม่มีชีตนี้"),
    ("the file's content could not be read", "อ่านเนื้อหาของไฟล์ไม่ได้"),
    // ---- the table's delimiters ----
    ("Comma (,)", "คอมมา (,)"),
    ("Tab", "Tab"),
    ("Semicolon (;)", "เซมิโคลอน (;)"),
    ("Pipe (|)", "ขีดตั้ง (|)"),
    // ---- why a sync leaves an item alone ----
    ("the right is newer", "ฝั่งขวาใหม่กว่า"),
    ("the left is newer", "ฝั่งซ้ายใหม่กว่า"),
    (
        "the modified times are equal or near: can't tell which is newer",
        "เวลาแก้ไขเท่ากันหรือใกล้กันมาก บอกไม่ได้ว่าฝั่งไหนใหม่กว่า",
    ),
    (
        "the names differ only in case and the contents match: rename it by hand",
        "ชื่อต่างแค่ตัวพิมพ์เล็กใหญ่ และเนื้อหาตรงกัน ให้เปลี่ยนชื่อเอง",
    ),
    (
        "holds files that are not in the compare (filtered out, links or unreadable): the folder stays",
        "มีไฟล์ที่ไม่อยู่ในการเทียบ (ถูกกรองออก, ลิงก์ หรืออ่านไม่ได้) จึงเก็บโฟลเดอร์ไว้",
    ),
    (
        "it changed on disk since the compare",
        "เปลี่ยนไปบนดิสก์หลังเทียบ",
    ),
    ("it could not be read", "อ่านไม่ได้"),
    (
        "a file on one side and a folder or link on the other",
        "ฝั่งหนึ่งเป็นไฟล์ อีกฝั่งเป็นโฟลเดอร์หรือลิงก์",
    ),
    ("links are never copied", "ไม่คัดลอกลิงก์"),
    (
        "the names differ only in case and the size and modified time match: rename it by hand",
        "ชื่อต่างแค่ตัวพิมพ์เล็กใหญ่ และขนาดกับเวลาแก้ไขตรงกัน ให้เปลี่ยนชื่อเอง",
    ),
    (
        "the names differ only in case and the size matches: rename it by hand",
        "ชื่อต่างแค่ตัวพิมพ์เล็กใหญ่ และขนาดตรงกัน ให้เปลี่ยนชื่อเอง",
    ),
    (
        "one folder is inside the other, so nothing was synced",
        "โฟลเดอร์หนึ่งอยู่ในอีกโฟลเดอร์ จึงไม่ได้ซิงก์อะไร",
    ),
    // ---- how a file is stored ----
    ("CRLF (Windows)", "CRLF (Windows)"),
    ("LF (Linux)", "LF (Linux)"),
    ("CR (old Mac)", "CR (Mac รุ่นเก่า)"),
    ("one line", "บรรทัดเดียว"),
    (", no final newline", ", ไม่ขึ้นบรรทัดท้ายไฟล์"),
    (", BOM", ", BOM"),
    // ---- sentences core writes ----
    (
        "Cannot compare a file with a folder.",
        "เทียบไฟล์กับโฟลเดอร์ไม่ได้",
    ),
    ("cannot read the link target", "อ่านปลายทางของลิงก์ไม่ได้"),
    ("Type some text to find.", "พิมพ์ข้อความที่จะค้นหา"),
    ("Type hex bytes, such as 4D 5A.", "พิมพ์ไบต์ hex เช่น 4D 5A"),
    ("invalid pattern", "รูปแบบไม่ถูกต้อง"),
    ("outside the compared folder", "อยู่นอกโฟลเดอร์ที่เทียบ"),
    // ---- errors ----
    ("operation cancelled", "ยกเลิกแล้ว"),
    ("{} already exists", "{} มีอยู่แล้ว"),
    ("{}: {}", "{}: {}"),
    (
        "cannot move {} to the Recycle Bin: {}",
        "ย้าย {} ไปถังขยะไม่ได้: {}",
    ),
    ("Windows would not allow it", "Windows ไม่อนุญาต"),
    (
        "{} is a link or junction; duet never writes through links",
        "{} เป็นลิงก์หรือ junction และ duet ไม่เขียนผ่านลิงก์",
    ),
    (
        "cannot copy {} into itself ({})",
        "คัดลอก {} ไปไว้ในตัวมันเองไม่ได้ ({})",
    ),
    ("Changed since the compare.", "เปลี่ยนไปตั้งแต่ตอนเทียบ"),
    ("invalid filter pattern: {}", "รูปแบบตัวกรองไม่ถูกต้อง: {}"),
    (
        "{} is not a readable folder: {}",
        "{} ไม่ใช่โฟลเดอร์ที่อ่านได้: {}",
    ),
    ("{} is not a folder", "{} ไม่ใช่โฟลเดอร์"),
    (
        "cannot start worker threads: {}",
        "เริ่ม thread ทำงานไม่ได้: {}",
    ),
    ("the file is larger than {} MB.", "ไฟล์ใหญ่เกิน {} MB"),
    ("{} is not supported.", "ไม่รองรับ {}"),
    (
        "the picture can't be decoded: the format is not supported, or the file is damaged.",
        "เปิดภาพไม่ได้ อาจเพราะไม่รองรับรูปแบบนี้ หรือไฟล์เสียหาย",
    ),
    ("the picture has no size.", "ภาพนี้ไม่มีขนาด"),
    (
        "the picture is too large to open ({}×{}).",
        "ภาพใหญ่เกินกว่าจะเปิด ({}×{})",
    ),
    ("it is a binary file", "เป็นไฟล์ไบนารี"),
    ("cannot read {}: {}", "อ่าน {} ไม่ได้: {}"),
    ("{} is a binary file", "{} เป็นไฟล์ไบนารี"),
    (
        "{} is binary and over 64 MiB, too large to open as text.",
        "{} เป็นไฟล์ไบนารี และใหญ่เกิน 64 MiB เปิดเป็นข้อความไม่ได้",
    ),
    (
        "{} changed on disk since it was loaded",
        "{} ถูกเปลี่ยนบนดิสก์หลังจากเปิด",
    ),
    ("cannot save {}: {}", "บันทึก {} ไม่ได้: {}"),
    (
        "text contains a character that {} cannot represent (byte offset {})",
        "ข้อความมีอักขระที่ {} เก็บไม่ได้ (offset ไบต์ที่ {})",
    ),
    ("too many lines to diff ({})", "มีบรรทัดมากเกินกว่าจะเทียบ ({})"),
];
