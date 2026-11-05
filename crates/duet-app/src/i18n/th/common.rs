//! Thai used all over the UI.
//!
//! The terms, one word per concept, the same in the UI, the Help and the reports: compare เทียบ,
//! side ฝั่ง, left/right ซ้าย/ขวา in a label and ฝั่งซ้าย/ฝั่งขวา in a sentence, both sides ทั้งสองฝั่ง,
//! base ฐาน, same เหมือนกัน, match ตรงกัน, difference จุดต่าง (a place) and ความต่าง (the fact),
//! changed เปลี่ยน, a find's match จุดที่พบ, line ending ท้ายบรรทัด, final newline ขึ้นบรรทัดท้ายไฟล์,
//! unimportant ไม่สำคัญ, ignore ไม่สนใจ, skip ข้าม, leading ต้นบรรทัด, indentation ย่อหน้า, mask ซ่อนค่า,
//! fingerprint รหัสย่อ, sync ซิงก์, preview พรีวิว, report รายงาน, export ส่งออก, overwrite เขียนทับ,
//! Recycle Bin ถังขยะ, case ตัวพิมพ์เล็กใหญ่ (ตัวพิมพ์ in a tight label), criterion เกณฑ์, tolerance
//! ค่าคลาดเคลื่อน, delimiter ตัวคั่น, key column คอลัมน์คีย์, header หัวตาราง, sheet ชีต, 3-way merge
//! รวมสามทาง, merge result ผลการรวม, binary ไบนารี, link ลิงก์, picture รูปภาพ (ภาพ in a sentence),
//! failed ไม่สำเร็จ, expand ขยาย, collapse ยุบ, shortcuts ปุ่มลัด.
//!
//! Kept in Latin, as the Help does: conflict, conflict marker, offset, hex, regex, path, patch, tab
//! (the character), and every format and encoding name.
//!
//! Labels are short: a verb and its object, no ครับ, no กรุณา, no "ทำการ". Sentences are plain, with
//! a space at each phrase break. Copy exactly from the key: `(F5)`-style key hints, `…`, holes. A
//! `count_of` noun is the plural key's Thai with no number word.

pub const ENTRIES: &[(&str, &str)] = &[
    // ---- count_of nouns ----
    ("files", "ไฟล์"),
    ("lines", "บรรทัด"),
    // ---- prompt answers every prompt may hold ----
    ("OK", "ตกลง"),
    ("Cancel", "ยกเลิก"),
];
