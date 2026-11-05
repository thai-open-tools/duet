//! The window around the views: the header, the tabs, the path bars, the recent list, the legend,
//! the field menu and what the command line says.

pub const ENTRIES: &[(&str, &str)] = &[
    // ---- header ----
    ("Works offline · 100% local", "ทำงานออฟไลน์ · ในเครื่อง 100%"),
    ("Help (F1)", "คู่มือ (F1)"),
    ("Switch between light and dark", "สลับธีมสว่างและธีมมืด"),
    // ---- tabs ----
    ("Close tab (Ctrl+W)", "ปิดแท็บ (Ctrl+W)"),
    ("New compare (Ctrl+T)", "เทียบใหม่ (Ctrl+T)"),
    ("untitled", "ไม่มีชื่อ"),
    ("Start", "เริ่มต้น"),
    ("Help", "คู่มือ"),
    ("3-way merge", "รวมสามทาง"),
    ("Merge: {} ↔ {}", "รวม: {} ↔ {}"),
    ("ok", "เหมือนกัน"),
    ("Binary files are identical.", "ไฟล์ไบนารีเหมือนกัน"),
    ("Binary files differ.", "ไฟล์ไบนารีต่างกัน"),
    (
        "That pair is already open in another tab.",
        "คู่นี้เปิดอยู่ในอีกแท็บแล้ว",
    ),
    (
        "Saved. This pair is already open in another tab.",
        "บันทึกแล้ว แต่คู่นี้เปิดอยู่ในอีกแท็บแล้ว",
    ),
    // ---- path bars ----
    (
        "Type a full path, such as C:\\folder\\file.txt.",
        "พิมพ์ path เต็ม เช่น C:\\folder\\file.txt",
    ),
    (
        "Drop files here; folders open in a folder compare.",
        "วางไฟล์ที่นี่ ส่วนโฟลเดอร์จะเปิดเป็นการเทียบโฟลเดอร์",
    ),
    ("Not a file: {}.", "ไม่ใช่ไฟล์: {}"),
    ("Path", "path"),
    ("Copy path", "คัดลอก path"),
    // ---- recent list ----
    ("Folders", "โฟลเดอร์"),
    ("Files", "ไฟล์"),
    ("Hex", "Hex"),
    ("Table", "ตาราง"),
    ("Picture", "รูปภาพ"),
    ("Merge", "รวม"),
    (" · base {}", " · ฐาน {}"),
    // ---- legend ----
    ("Changed", "เปลี่ยน"),
    ("Left only", "เฉพาะซ้าย"),
    ("Right only", "เฉพาะขวา"),
    ("Moved", "ย้าย"),
    ("Unimportant", "ไม่สำคัญ"),
    // ---- a field's right-click menu ----
    ("Cut", "ตัด"),
    ("Copy", "คัดลอก"),
    ("Paste", "วาง"),
    ("Select all", "เลือกทั้งหมด"),
    // ---- the command line ----
    ("A 3-way merge needs three files.", "รวมสามทางต้องใช้สามไฟล์"),
    (
        "Cannot create the empty file for {}: {}",
        "สร้างไฟล์เปล่าสำหรับ {} ไม่ได้: {}",
    ),
    (
        "The output must be a file, not a folder.",
        "ปลายทางต้องเป็นไฟล์ ไม่ใช่โฟลเดอร์",
    ),
    ("--table needs two files.", "--table ต้องใช้สองไฟล์"),
    ("{}: no longer exists", "{}: หายไปแล้ว"),
];
