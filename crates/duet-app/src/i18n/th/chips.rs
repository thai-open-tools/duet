//! Help chips that no control carries under that exact label (D7): a chip drawn from a label with a
//! value in it, a short name for a long label, or an icon-only control's tooltip. The Help draws
//! each through `tr`, and `thai_chips_name_what_the_controls_show` holds them to the Thai of the
//! control they name.

pub const ENTRIES: &[(&str, &str)] = &[
    (".*", ".*"),
    ("1:1", "1:1"),
    ("Bytes per row: 16", "ไบต์ต่อแถว: 16"),
    ("Close tab", "ปิดแท็บ"),
    ("Delimiter: Auto", "ตัวคั่น: อัตโนมัติ"),
    ("Font 12.5", "ฟอนต์ 12.5"),
    (
        "Hide values that look like passwords…",
        "ซ่อนค่าที่ดูเหมือนรหัสผ่าน…",
    ),
    ("Swap sides", "สลับซ้าย-ขวา"),
    ("Tab 4", "Tab 4"),
    ("Type: Auto", "ชนิด: อัตโนมัติ"),
    ("Zoom in", "ซูมเข้า"),
    ("Zoom out", "ซูมออก"),
];
