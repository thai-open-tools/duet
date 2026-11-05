//! The hex view in Thai: the toolbar, the find bar, the status line, the menu and the notices.

pub const ENTRIES: &[(&str, &str)] = &[
    // ---- the toolbar ----
    ("Bytes per row: {}", "ไบต์ต่อแถว: {}"),
    ("Offset, e.g. 0x1F40", "Offset เช่น 0x1F40"),
    ("Go to offset", "ไปที่ offset"),
    ("Go to", "ไป"),
    ("Go to the offset (Enter)", "ไปที่ offset (Enter)"),
    ("Copy hex", "คัดลอก hex"),
    ("Copy hex (Ctrl+C)", "คัดลอกเป็น hex (Ctrl+C)"),
    // ---- the rows ----
    ("Offset", "Offset"),
    // ---- the find bar ----
    (
        "Enter: next match. Shift+Enter: previous match.",
        "Enter: จุดที่พบถัดไป Shift+Enter: จุดที่พบก่อนหน้า",
    ),
    ("Hex bytes", "ไบต์ hex"),
    ("Text", "ข้อความ"),
    ("Wrapped to the start of the file.", "วนกลับไปต้นไฟล์"),
    ("Wrapped to the end of the file.", "วนกลับไปท้ายไฟล์"),
    // ---- the status line ----
    ("{} ({} bytes)", "{} ({} ไบต์)"),
    ("Left: {}", "ซ้าย: {}"),
    ("Right: {}", "ขวา: {}"),
    (
        "Sizes differ by {} ({} is longer)",
        "ขนาดต่างกัน {} (ฝั่ง{}ยาวกว่า)",
    ),
    ("Comparing… {}%", "กำลังเทียบ… {}%"),
    ("Identical byte for byte.", "เหมือนกันทุกไบต์"),
    (
        "Different bytes: {} · Differences: {} ({})",
        "ไบต์ที่ต่าง: {} · จุดต่าง: {} ({})",
    ),
    ("runs", "ช่วง"),
    ("Searching… {}%", "กำลังค้น… {}%"),
    (
        "More than 200,000 differences: navigation covers the first 200,000.",
        "จุดต่างเกิน 200,000 จุด การไปจุดต่างครอบคลุมแค่ 200,000 จุดแรก",
    ),
    (
        "Offset 0x{} ({}) · left {} right {}",
        "Offset 0x{} ({}) · ซ้าย {} ขวา {}",
    ),
    // ---- the menu ----
    ("Copy bytes (hex)", "คัดลอกไบต์ (hex)"),
    ("Copy this row", "คัดลอกแถวนี้"),
    ("Copy offset", "คัดลอก offset"),
    ("Previous difference", "จุดต่างก่อนหน้า"),
    ("Next difference", "จุดต่างถัดไป"),
    ("Go to offset…", "ไปที่ offset…"),
    // ---- notices and the body ----
    ("Press F5 to try again.", "กด F5 เพื่อลองใหม่"),
    ("Still comparing…", "ยังเทียบไม่เสร็จ…"),
    (
        "Showing all rows to reach that offset.",
        "แสดงทุกแถวเพื่อไปถึง offset นั้น",
    ),
    (
        "Type an offset such as 0x1F40, 1F40h or 8000.",
        "พิมพ์ offset เช่น 0x1F40, 1F40h หรือ 8000",
    ),
    (
        "Past the end: went to the last byte.",
        "เกินท้ายไฟล์ จึงไปที่ไบต์สุดท้าย",
    ),
    (
        "Cancelled: the files were not opened.",
        "ยกเลิกแล้ว: ไม่ได้เปิดไฟล์",
    ),
    ("There are no bytes to go to.", "ไม่มีไบต์ให้ไปถึง"),
    (
        "Copied the first 1 MiB of the selection.",
        "คัดลอก 1 MiB แรกของส่วนที่เลือกแล้ว",
    ),
    ("Copied the cursor row.", "คัดลอกแถวของเคอร์เซอร์แล้ว"),
    (
        "Nothing to copy: click a byte or select a range first.",
        "ไม่มีอะไรให้คัดลอก คลิกไบต์หรือเลือกช่วงก่อน",
    ),
    (
        "The selection is past the end of the {} file.",
        "ส่วนที่เลือกเลยท้ายไฟล์ฝั่ง{}",
    ),
    ("Copied {}.", "คัดลอกแล้ว {}"),
];
