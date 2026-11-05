//! The picture view in Thai: the mode bar, zoom, tolerance, heads, status line and notices.

pub const ENTRIES: &[(&str, &str)] = &[
    // ---- the modes ----
    ("Side by side", "เคียงกัน"),
    ("Difference", "จุดต่าง"),
    ("Slider", "ลากเทียบ"),
    ("Blink", "กะพริบ"),
    (
        "Both pictures side by side: they pan and zoom together (1)",
        "วางสองภาพเคียงกัน เลื่อนและซูมไปพร้อมกัน (1)",
    ),
    (
        "Difference image: differing pixels in red on a dimmed copy of the left picture (2)",
        "ภาพจุดต่าง: พิกเซลที่ต่างเป็นสีแดง บนภาพซ้ายที่ลดความสว่าง (2)",
    ),
    (
        "One over the other: drag the divider to compare left and right (3)",
        "ซ้อนกัน ลากเส้นแบ่งเพื่อเทียบภาพซ้ายกับภาพขวา (3)",
    ),
    (
        "Show the left and right picture in turn every 0.6 seconds (4)",
        "สลับแสดงภาพซ้ายกับภาพขวาทุก 0.6 วินาที (4)",
    ),
    // ---- zoom, groups and files ----
    ("Fit", "พอดี"),
    ("Fit to the window (Ctrl+0)", "พอดีหน้าต่าง (Ctrl+0)"),
    ("Actual size, 100% (Ctrl+1)", "ขนาดจริง 100% (Ctrl+1)"),
    (
        "Zoom out (Ctrl+− or Ctrl+wheel)",
        "ซูมออก (Ctrl+− หรือ Ctrl+ล้อเมาส์)",
    ),
    (
        "Zoom in (Ctrl++ or Ctrl+wheel)",
        "ซูมเข้า (Ctrl++ หรือ Ctrl+ล้อเมาส์)",
    ),
    ("Previous difference group (F7)", "กลุ่มจุดต่างก่อนหน้า (F7)"),
    ("Next difference group (F8)", "กลุ่มจุดต่างถัดไป (F8)"),
    (
        "Read both pictures from disk again (F5)",
        "อ่านทั้งสองภาพจากดิสก์ใหม่ (F5)",
    ),
    ("Export difference image", "ส่งออกภาพจุดต่าง"),
    (
        "Save the difference image as a PNG file",
        "บันทึกภาพจุดต่างเป็นไฟล์ PNG",
    ),
    (
        "Pixels whose every channel (R, G, B, A) differs by this much or less count as the same",
        "พิกเซลที่ทุกช่อง (R, G, B, A) ต่างกันไม่เกินค่านี้ ถือว่าเหมือนกัน",
    ),
    ("Choose the left picture (Ctrl+O)", "เลือกภาพฝั่งซ้าย (Ctrl+O)"),
    (
        "Choose the right picture (Ctrl+Shift+O)",
        "เลือกภาพฝั่งขวา (Ctrl+Shift+O)",
    ),
    ("No picture chosen", "ยังไม่ได้เลือกภาพ"),
    ("Can't open", "เปิดไม่ได้"),
    ("Loading…", "กำลังโหลด…"),
    ("Loading {}…", "กำลังโหลด {}…"),
    (" and ", " และ "),
    // ---- the status line ----
    (
        "Open the other picture to start comparing.",
        "เปิดอีกภาพเพื่อเริ่มเทียบ",
    ),
    (
        "A picture is larger than 40 megapixels: it is shown, but pixels are not compared.",
        "ภาพใหญ่กว่า 40 ล้านพิกเซล แสดงได้แต่ไม่เทียบพิกเซล",
    ),
    (
        "Every pixel of the overlap is the same.",
        "ทุกพิกเซลในส่วนที่ซ้อนกันเหมือนกัน",
    ),
    ("Every pixel is the same.", "ทุกพิกเซลเหมือนกัน"),
    ("Same within the tolerance", "เหมือนกันภายในค่าคลาดเคลื่อน"),
    ("{}, largest difference {}", "{} ต่างมากสุด {}"),
    ("pixels differ slightly", "พิกเซลต่างเล็กน้อย"),
    ("pixels differ", "พิกเซลต่างกัน"),
    ("{} more within the tolerance", "อีก {} อยู่ในค่าคลาดเคลื่อน"),
    ("Group {} / {}", "กลุ่ม {} / {}"),
    (
        "Sizes differ ({}×{} and {}×{}): only the {}×{} overlap is compared.",
        "ขนาดต่างกัน ({}×{} กับ {}×{}) เทียบเฉพาะส่วนที่ซ้อนกัน {}×{}",
    ),
    ("Tolerance {} · {}", "ค่าคลาดเคลื่อน {} · {}"),
    ("computing…", "กำลังคำนวณ…"),
    ("Not the same size as the other side", "ขนาดไม่เท่ากับอีกฝั่ง"),
    ("Modified {}", "แก้ไขเมื่อ {}"),
    ("{}, {} · Left", "{}, {} · ซ้าย"),
    ("· Right", "· ขวา"),
    ("{}, {} · Left {} · Right {}", "{}, {} · ซ้าย {} · ขวา {}"),
    ("Within tolerance", "ในค่าคลาดเคลื่อน"),
    ("One side only", "มีฝั่งเดียว"),
    // ---- the stage ----
    (
        "A picture is larger than 40 megapixels: no difference image.",
        "ภาพใหญ่กว่า 40 ล้านพิกเซล จึงไม่สร้างภาพจุดต่าง",
    ),
    ("Open both pictures first.", "เปิดทั้งสองภาพก่อน"),
    (
        "No differences above the tolerance.",
        "ไม่มีจุดต่างเกินค่าคลาดเคลื่อน",
    ),
    (
        "No comparison yet: press F5 to compare.",
        "ยังไม่ได้เทียบ กด F5 เพื่อเทียบ",
    ),
    ("◀ Left", "◀ ซ้าย"),
    ("Right ▶", "ขวา ▶"),
    ("Drop as the left picture", "วางเป็นภาพซ้าย"),
    ("Drop as the right picture", "วางเป็นภาพขวา"),
    // ---- the menu ----
    ("Fit to window", "พอดีหน้าต่าง"),
    ("Actual size (1:1)", "ขนาดจริง (1:1)"),
    ("Swap left and right", "สลับซ้าย-ขวา"),
    ("Export difference image (PNG)…", "ส่งออกภาพจุดต่าง (PNG)…"),
    // ---- notices ----
    ("Pictures reloaded.", "โหลดภาพใหม่แล้ว"),
    ("No pictures to reload.", "ไม่มีภาพให้โหลดใหม่"),
    (
        "No difference can be computed for these pictures.",
        "คำนวณจุดต่างของภาพเหล่านี้ไม่ได้",
    ),
    (
        "{} files dropped: the first two are used.",
        "วางมา {} ไฟล์ ใช้แค่สองไฟล์แรก",
    ),
];
