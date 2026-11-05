//! The Start tab: the path form, the six tiles, the recent list and the short sheet of keys.
//!
//! The sheet's own rows read `SHORTCUTS_TH`, not this table.

pub const ENTRIES: &[(&str, &str)] = &[
    // ---- the form ----
    ("New compare", "เทียบใหม่"),
    ("Left", "ซ้าย"),
    ("Right", "ขวา"),
    ("Left file or folder", "ไฟล์หรือโฟลเดอร์ฝั่งซ้าย"),
    ("Right file or folder", "ไฟล์หรือโฟลเดอร์ฝั่งขวา"),
    ("Left path", "path ฝั่งซ้าย"),
    ("Right path", "path ฝั่งขวา"),
    ("File…", "ไฟล์…"),
    ("Folder…", "โฟลเดอร์…"),
    (
        "Drop two files or folders here, or type their paths",
        "วางไฟล์หรือโฟลเดอร์สองรายการที่นี่ หรือพิมพ์ path",
    ),
    (
        "Two empty sides to type or paste into; you can open files into either side later",
        "เปิดสองฝั่งที่ว่าง ไว้พิมพ์หรือวางข้อความ จะเปิดไฟล์เข้าฝั่งไหนทีหลังก็ได้",
    ),
    ("Paste text…", "วางข้อความ…"),
    ("Compare", "เทียบ"),
    (
        "Choose two files or two folders.",
        "เลือกไฟล์สองไฟล์ หรือโฟลเดอร์สองโฟลเดอร์",
    ),
    // ---- the tiles ----
    ("Or pick a type", "หรือเลือกชนิดที่จะเทียบ"),
    ("Compare files", "เทียบไฟล์"),
    (
        "Two text files side by side: changes, moved lines, edits and saves.",
        "ไฟล์ข้อความสองไฟล์เทียบกันสองฝั่ง ดูจุดที่เปลี่ยนและบรรทัดที่ย้าย แก้ไขแล้วบันทึกได้",
    ),
    ("Compare folders", "เทียบโฟลเดอร์"),
    (
        "Two folder trees: what differs, what is missing, copy and delete.",
        "สองโฟลเดอร์: อะไรต่างกัน อะไรขาดไป พร้อมคัดลอกและลบ",
    ),
    (
        "Combine two edited copies of a file with their common original.",
        "รวมไฟล์ที่แก้ไขมาสองฉบับ เข้ากับต้นฉบับที่ใช้ร่วมกัน",
    ),
    ("Compare binary files", "เทียบไฟล์ไบนารี"),
    (
        "Byte by byte, side by side, for any file.",
        "ทีละไบต์ วางเทียบกันสองฝั่ง ใช้ได้กับทุกไฟล์",
    ),
    ("Compare tables", "เทียบตาราง"),
    (
        "CSV, TSV or Excel (.xlsx) sheets, cell by cell: rows paired by key or by order.",
        "CSV, TSV หรือชีต Excel (.xlsx) ทีละเซลล์ จับคู่แถวตามคีย์หรือตามลำดับ",
    ),
    ("Compare pictures", "เทียบรูปภาพ"),
    (
        "PNG, JPEG, GIF, BMP, WebP, ICO, TIFF or SVG, pixel by pixel.",
        "PNG, JPEG, GIF, BMP, WebP, ICO, TIFF หรือ SVG ทีละพิกเซล",
    ),
    // ---- the recent list ----
    ("Recent", "ล่าสุด"),
    ("Clear", "ล้าง"),
    ("Compared pairs show up here.", "คู่ที่เทียบแล้วจะแสดงที่นี่"),
    ("{}: {} and {}", "{}: {} และ {}"),
    ("{}: {}, {} and {}", "{}: {}, {} และ {}"),
    ("Show all ({})", "แสดงทั้งหมด ({})"),
    ("Show fewer", "แสดงน้อยลง"),
    ("Open", "เปิด"),
    ("Remove from list", "เอาออกจากรายการ"),
    ("Remove from the list", "เอาออกจากรายการ"),
    // ---- the sheet of keys ----
    ("Keyboard shortcuts", "ปุ่มลัด"),
    ("All shortcuts in Help (F1)", "ดูปุ่มลัดทั้งหมดในคู่มือ (F1)"),
];
