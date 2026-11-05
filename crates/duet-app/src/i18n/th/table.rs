//! The table view in Thai: toolbars, status, the detail strip, find and go to, menus and notices.
//!
//! The row filter chips (`All`, `Differences`, `Same`, `Left only`, `Right only`) come from core and
//! sit in the other areas' tables; the delimiter names are in `core_text.rs`.

pub const ENTRIES: &[(&str, &str)] = &[
    // ---- the toolbar ----
    ("Open left…", "เปิดซ้าย…"),
    ("Open right…", "เปิดขวา…"),
    ("Open the left file (Ctrl+O)", "เปิดไฟล์ฝั่งซ้าย (Ctrl+O)"),
    (
        "Open the right file (Ctrl+Shift+O)",
        "เปิดไฟล์ฝั่งขวา (Ctrl+Shift+O)",
    ),
    (
        "Paste the clipboard as the left table",
        "วางจากคลิปบอร์ดเป็นตารางฝั่งซ้าย",
    ),
    (
        "Paste the clipboard as the right table",
        "วางจากคลิปบอร์ดเป็นตารางฝั่งขวา",
    ),
    ("First differing row (Ctrl+Home)", "แถวแรกที่ต่าง (Ctrl+Home)"),
    ("Previous differing row (F7)", "แถวที่ต่างก่อนหน้า (F7)"),
    ("Next differing row (F8)", "แถวที่ต่างถัดไป (F8)"),
    ("Last differing row (Ctrl+End)", "แถวสุดท้ายที่ต่าง (Ctrl+End)"),
    ("Find…", "ค้นหา…"),
    ("Export", "ส่งออก"),
    ("Export the differences", "ส่งออกจุดต่าง"),
    // ---- the options row ----
    ("Delimiter: {}", "ตัวคั่น: {}"),
    ("First row is a header", "แถวแรกเป็นหัวตาราง"),
    ("Treat the first row as column names", "ใช้แถวแรกเป็นชื่อคอลัมน์"),
    ("Ignore case", "ไม่สนใจตัวพิมพ์"),
    (
        "Compare text without regard to upper and lower case",
        "เทียบข้อความโดยไม่สนใจตัวพิมพ์เล็กใหญ่",
    ),
    ("Trim spaces", "ตัดช่องว่าง"),
    (
        "Ignore spaces at the start and end of a cell",
        "ไม่สนใจช่องว่างต้นและท้ายข้อความในเซลล์",
    ),
    ("Compare numbers by value", "เทียบตัวเลขตามค่า"),
    ("Key columns", "คอลัมน์คีย์"),
    (
        "Column names or 1-based numbers, comma-separated. Empty: rows align by order.",
        "ชื่อคอลัมน์หรือเลขลำดับ (เริ่มที่ 1) คั่นด้วย , ถ้าเว้นว่างจะจับคู่แถวตามลำดับ",
    ),
    (
        "e.g. id or 1,3 (empty = row order)",
        "เช่น id หรือ 1,3 (ว่าง = ตามลำดับแถว)",
    ),
    // ---- find and go to ----
    ("Previous (Shift+F3)", "ก่อนหน้า (Shift+F3)"),
    ("Next (F3)", "ถัดไป (F3)"),
    ("Find in every cell, both sides", "ค้นหาในทุกเซลล์ของทั้งสองฝั่ง"),
    (
        "Line in the file (the header is line 1)",
        "บรรทัดในไฟล์ (หัวตารางคือบรรทัดที่ 1)",
    ),
    ("Type a line number.", "พิมพ์เลขบรรทัด"),
    (
        "Still searching: press F3 to go on.",
        "ยังค้นไม่จบ กด F3 เพื่อค้นต่อ",
    ),
    ("Not found: {}.", "ไม่พบ: {}"),
    ("No row at line {}.", "ไม่มีแถวที่บรรทัด {}"),
    // ---- the grid ----
    ("Key", "คีย์"),
    (" (key column)", " (คอลัมน์คีย์)"),
    ("Only on the left: not compared.", "มีเฉพาะฝั่งซ้าย ไม่ได้เทียบ"),
    ("Only on the right: not compared.", "มีเฉพาะฝั่งขวา ไม่ได้เทียบ"),
    ("cells differ", "เซลล์ต่างกัน"),
    // ---- the detail strip ----
    (
        "Click a cell to see both values in full · N / P or ◀ ▶ move between differing rows · Right-click a column header to use it as the key.",
        "คลิกเซลล์เพื่อดูค่าทั้งสองฝั่งแบบเต็ม · N / P หรือ ◀ ▶ ไปแถวที่ต่าง · คลิกขวาที่หัวคอลัมน์เพื่อใช้เป็นคีย์",
    ),
    ("Differs", "ต่างกัน"),
    ("(none)", "(ไม่มี)"),
    ("characters {}–{} of {}", "อักขระ {}–{} จาก {}"),
    ("Left row {} · Right row {}", "แถวซ้าย {} · แถวขวา {}"),
    (" · Key {}", " · คีย์ {}"),
    (" · Column {}", " · คอลัมน์ {}"),
    (" (left only)", " (เฉพาะซ้าย)"),
    (" (right only)", " (เฉพาะขวา)"),
    // ---- the status line and notices ----
    (
        "Open the other side to start comparing.",
        "เปิดอีกฝั่งเพื่อเริ่มเทียบ",
    ),
    (
        "{} could not be read. Open another file to start comparing.",
        "อ่าน {} ไม่ได้ เปิดไฟล์อื่นเพื่อเริ่มเทียบ",
    ),
    (
        "The tables are identical cell for cell.",
        "ตารางเหมือนกันทุกเซลล์",
    ),
    ("Aligned by row order", "จับคู่ตามลำดับแถว"),
    (" Rows are aligned by order.", " จับคู่แถวตามลำดับ"),
    ("Differ {}", "ต่างกัน {}"),
    ("Cells differ {}", "เซลล์ที่ต่าง {}"),
    ("Columns matched {}", "คอลัมน์ที่ตรงกัน {}"),
    (" · left only {}", " · เฉพาะซ้าย {}"),
    (" · right only {}", " · เฉพาะขวา {}"),
    ("Paired by key: {}", "จับคู่ด้วยคีย์: {}"),
    (" · duplicate keys {}", " · คีย์ซ้ำ {}"),
    ("Key column not found: {}", "ไม่พบคอลัมน์คีย์: {}"),
    (
        "Key column not found on both sides: {}.",
        "ไม่พบคอลัมน์คีย์ในทั้งสองฝั่ง: {}",
    ),
    (
        "No column names match. Turn off “First row is a header” to compare by position.",
        "ไม่มีชื่อคอลัมน์ที่ตรงกัน ปิด “แถวแรกเป็นหัวตาราง” เพื่อเทียบตามตำแหน่ง",
    ),
    ("{} · {} · Delimiter: {}", "{} · {} · ตัวคั่น: {}"),
    ("{} rows × {} columns", "{} แถว × {} คอลัมน์"),
    ("{}: {} · {}", "{}: {} · {}"),
    ("Parsing {}… {} rows", "กำลังอ่าน {}… {} แถว"),
    ("Pasted text ({})", "ข้อความที่วาง ({})"),
    ("the file is larger than 160 MB.", "ไฟล์ใหญ่เกิน 160 MB"),
    (
        "“{}” looks like a binary file, not CSV or TSV.",
        "“{}” ดูเหมือนไฟล์ไบนารี ไม่ใช่ CSV หรือ TSV",
    ),
    (
        "The “Same” filter has no differing rows.",
        "ตัวกรอง “เหมือนกัน” ไม่มีแถวที่ต่าง",
    ),
    ("No differing rows.", "ไม่มีแถวที่ต่าง"),
    ("No more differences.", "ไม่มีจุดต่างถัดไปแล้ว"),
    ("No earlier differences.", "ไม่มีจุดต่างก่อนหน้านี้"),
    ("Copied.", "คัดลอกแล้ว"),
    // ---- Excel workbooks: the sheet picker, the status, the refusals and the notes ----
    ("Sheet: {}", "ชีต: {}"),
    ("{} (hidden)", "{} (ซ่อน)"),
    (
        "Choose the sheet (Ctrl+PageUp / Ctrl+PageDown; with Shift for the right side)",
        "เลือกชีต (Ctrl+PageUp / Ctrl+PageDown; กด Shift ด้วยสำหรับฝั่งขวา)",
    ),
    ("Applies to the CSV side only", "ใช้กับฝั่ง CSV เท่านั้น"),
    ("sheet “{}” ({} of {})", "ชีต “{}” ({} จาก {})"),
    ("{} · sheet “{}” ({} of {})", "{} · ชีต “{}” ({} จาก {})"),
    ("{} (sheet “{}”)", "{} (ชีต “{}”)"),
    (
        "is password protected. duet cannot open protected workbooks: remove the password in Excel and save a copy.",
        "มีรหัสผ่านป้องกันอยู่ duet เปิดไฟล์ที่มีรหัสผ่านไม่ได้ ให้เอารหัสผ่านออกใน Excel แล้วบันทึกเป็นสำเนา",
    ),
    (
        "is an old Excel file (.xls) saved under an .xlsx name.",
        "เป็นไฟล์ Excel รุ่นเก่า (.xls) ที่ตั้งนามสกุลเป็น .xlsx",
    ),
    (
        "is not an Excel workbook (it is not a zip file).",
        "ไม่ใช่ไฟล์ Excel (ไม่ใช่ไฟล์ zip)",
    ),
    (
        "is a zip file but not an Excel workbook.",
        "เป็นไฟล์ zip แต่ไม่ใช่ไฟล์ Excel",
    ),
    ("has no worksheets.", "ไม่มีชีตเลย"),
    ("is damaged: {}.", "ไฟล์เสียหาย: {}"),
    ("is damaged and can't be read.", "ไฟล์เสียหาย อ่านไม่ได้"),
    (
        "holds an Excel 4 macro sheet this version cannot open.",
        "มีชีตแมโคร Excel 4 ที่ duet รุ่นนี้ยังเปิดไม่ได้",
    ),
    (
        "unpacks to more than 2 GB, more than duet reads.",
        "แตกไฟล์แล้วใหญ่เกิน 2 GB เกินกว่าที่ duet อ่านได้",
    ),
    (
        "holds a part that unpacks to more than 1 GB, more than duet reads.",
        "มีส่วนที่แตกแล้วใหญ่เกิน 1 GB เกินกว่าที่ duet อ่านได้",
    ),
    (
        "is packed more tightly than a real workbook (over 1000 to 1). It may be damaged or built to crash a reader.",
        "ถูกบีบอัดแน่นเกินไฟล์ Excel จริง (เกิน 1000 ต่อ 1) ไฟล์อาจเสียหาย หรือถูกสร้างมาให้โปรแกรมอ่านแล้วล่ม",
    ),
    (
        "says it holds more text than its size allows. It may be damaged or built to crash a reader.",
        "ระบุว่ามีข้อความมาก เกินกว่าขนาดไฟล์จะมีได้ ไฟล์อาจเสียหาย หรือถูกสร้างมาให้โปรแกรมอ่านแล้วล่ม",
    ),
    (
        "Can't read sheet “{}” in {}: {}",
        "อ่านชีต “{}” ใน {} ไม่ได้: {}",
    ),
    (
        "this sheet holds a cell error this version cannot read ({}).",
        "ชีตนี้มีเซลล์ค่าผิดพลาดแบบที่ duet รุ่นนี้ยังอ่านไม่ได้ ({})",
    ),
    ("the sheet is damaged: {}.", "ชีตเสียหาย: {}"),
    (
        "the sheet is damaged and can't be read.",
        "ชีตเสียหาย อ่านไม่ได้",
    ),
    ("Choose another sheet.", "เลือกชีตอื่น"),
    // ---- what a result of a workbook pair cannot speak for ----
    (
        "Sheet “{}” is identical cell for cell.",
        "ชีต “{}” เหมือนกันทุกเซลล์",
    ),
    (
        "“{}” and “{}” are identical cell for cell.",
        "“{}” กับ “{}” เหมือนกันทุกเซลล์",
    ),
    (
        "The rows read are identical. The rest of the sheet was not read.",
        "แถวที่อ่านมาเหมือนกัน แต่ส่วนที่เหลือของชีตยังไม่ได้อ่าน",
    ),
    (
        "The values read are identical. Formula cells with no saved result were not compared.",
        "ค่าที่อ่านมาเหมือนกัน แต่เซลล์สูตรที่ไม่มีผลลัพธ์บันทึกไว้ ไม่ได้เทียบ",
    ),
    ("1 other sheet was not compared.", "อีก 1 ชีตไม่ได้เทียบ"),
    ("{} other sheets were not compared.", "อีก {} ชีตไม่ได้เทียบ"),
    ("Only in {}: {}.", "มีเฉพาะใน {}: {}"),
    (
        "The workbook side compares its stored values, not what Excel shows: a 25% cell is 0.25, and dates read 2024-03-05.",
        "ฝั่ง Excel เทียบค่าที่เก็บไว้จริง ไม่ใช่ค่าที่ Excel แสดง: เซลล์ 25% คือ 0.25 และวันที่อ่านเป็น 2024-03-05",
    ),
    (
        "A leading ' marks text that reads like a number, date or other value, as in Excel.",
        "เครื่องหมาย ' นำหน้าคือข้อความที่หน้าตาเหมือนตัวเลข วันที่ หรือค่าอื่น เหมือนใน Excel",
    ),
    (
        "is open in another program that does not let it be read. Close it or save a copy.",
        "ถูกเปิดอยู่ ในโปรแกรมอื่นที่ไม่ให้อ่านไฟล์ ปิดโปรแกรมนั้น หรือบันทึกเป็นสำเนา",
    ),
    (
        "Sheet “{}” is no longer in {}. Showing “{}”.",
        "ไม่มีชีต “{}” ใน {} แล้ว แสดงชีต “{}” แทน",
    ),
    (
        "Only the first {} rows of “{}” in {} were read (the sheet is larger than duet reads).",
        "อ่านเฉพาะ {} แถวแรกของ “{}” ใน {} (ชีตใหญ่เกินที่ duet อ่านได้)",
    ),
    (
        "1 formula cell has no saved result, so it shows empty.",
        "มี 1 เซลล์สูตรที่ไม่มีผลลัพธ์ที่บันทึกไว้ จึงแสดงว่าง",
    ),
    (
        "{} formula cells have no saved result, so they show empty.",
        "มี {} เซลล์สูตรที่ไม่มีผลลัพธ์ที่บันทึกไว้ จึงแสดงว่าง",
    ),
    (
        "Formula cells have no saved result, so they show empty: left {}, right {}.",
        "เซลล์สูตรไม่มีผลลัพธ์ที่บันทึกไว้ จึงแสดงว่าง: ซ้าย {} ขวา {}",
    ),
    // ---- the menus ----
    ("Copy left value", "คัดลอกค่าฝั่งซ้าย"),
    ("Copy right value", "คัดลอกค่าฝั่งขวา"),
    ("Copy left row (CSV)", "คัดลอกแถวซ้าย (CSV)"),
    ("Copy right row (CSV)", "คัดลอกแถวขวา (CSV)"),
    ("Use “{}” as the key column", "ใช้ “{}” เป็นคอลัมน์คีย์"),
    ("Add “{}” to the key columns", "เพิ่ม “{}” ในคอลัมน์คีย์"),
    (
        "Clear key columns (align by row order)",
        "ล้างคอลัมน์คีย์ (จับคู่ตามลำดับแถว)",
    ),
    ("Differences (CSV)", "จุดต่าง (CSV)"),
    ("HTML report", "รายงาน HTML"),
];
