# Contributing to duet

**English** · [ภาษาไทย](#ภาษาไทย)

Thank you for helping. Bug reports, ideas, documentation fixes and code are all welcome, in English or Thai.

## Before you write code

Open a card first ([bug](https://github.com/ball46/duet/issues/new?template=bug_report.yml) or
[feature](https://github.com/ball46/duet/issues/new?template=feature_request.yml)) and say you would like
to work on it. A short talk about the approach saves a rewrite later. Small fixes (a typo, an obvious bug)
can go straight to a pull request.

## Set up

1. Windows 10 or 11, x64.
2. [Rust](https://rustup.rs/) 1.90 or later with the MSVC toolchain (`rustup default stable-x86_64-pc-windows-msvc`).
3. The Visual Studio Build Tools with the *Desktop development with C++* workload, which includes the Windows SDK.
4. For `scripts\check.ps1`: `cargo install cargo-deny`.

```powershell
git clone https://github.com/ball46/duet.git
cd duet
cargo run --release -p duet-app          # the app
cargo run --release -p duet-cli -- a b   # the command line
```

On a PC with 16 GB of memory or less, limit parallel jobs so the build and tests do not run out of memory:
`$env:CARGO_BUILD_JOBS = 2; $env:RUST_TEST_THREADS = 2`.

## The checks

`powershell -File scripts\check.ps1` runs what a pull request needs to pass:

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace` (about 1,750 tests; the UI tests drive the real views without opening a window)
- `cargo deny check bans licenses sources`

Performance matters in duet. If you touch the diff engine, the folder scan or scrolling, run
`scripts\bench.ps1` before and after your change and put both numbers in the pull request.

## House rules

- **Offline, always.** duet never talks to the network. Do not add crates that do HTTP or telemetry; `deny.toml` bans them and a test checks the app wires up no HTTP client.
- **Tests with the change.** A bug fix comes with a test that fails without it. A new behaviour comes with tests for it.
- **Small, focused pull requests.** One topic each; no drive-by reformatting.
- **UTF-8 source files.** Edit with an editor or the tools in `scripts\`, not with Windows PowerShell 5.1's `Set-Content`, which rewrites UTF-8 as another code page (`crates/duet-app/tests/source_hygiene.rs` catches it).
- **Words in the UI** are short and plain. If you change a label, change the guide too, in both `crates/duet-app/src/help/en.rs` and `th.rs`.
- **Commit messages** follow [Conventional Commits](https://www.conventionalcommits.org/): `feat:`, `fix:`, `docs:`, `refactor:`, `test:`, `perf:`, `chore:`.

## Where things are

| Path | What it holds |
|---|---|
| `crates/duet-core` | The compare engine, with no UI. Start here for diff, folder, merge, table, picture and hex logic. |
| `crates/duet-app` | The desktop app (gpui). `src/folder`, `src/text`, `src/table`, `src/picture`, `src/hex`, `src/merge` hold the views; `src/help` the guide. |
| `crates/duet-cli` | The terminal front end. |
| `installer/` | The Inno Setup script and the pictures it shows (`make-art.py`). |
| `scripts/` | Checks, benchmarks, screenshots, install, release. |

Comments in the code often mention **LocalDiff**: the author's earlier compare app, written in Electron.
duet replaced it, so its features and look were the yardstick.

## License

By sending a contribution you agree that it is released under the project's [MIT License](LICENSE).

---

## ภาษาไทย

ขอบคุณที่อยากช่วยพัฒนา duet ยินดีรับทั้งการแจ้งบั๊ก ไอเดีย การแก้เอกสาร และโค้ด จะเขียนเป็นภาษาไทยหรืออังกฤษก็ได้

### ก่อนเริ่มเขียนโค้ด

ให้เปิด issue ก่อน ([บั๊ก](https://github.com/ball46/duet/issues/new?template=bug_report.yml) หรือ
[ฟีเจอร์](https://github.com/ball46/duet/issues/new?template=feature_request.yml)) แล้วบอกว่าอยากทำเรื่องนี้
คุยแนวทางกันสั้นๆ ก่อนจะช่วยไม่ให้ต้องรื้อทำใหม่ ส่วนการแก้เล็กๆ เช่น คำผิดหรือบั๊กที่เห็นชัด ส่ง pull request มาได้เลย

### เตรียมเครื่อง

1. Windows 10 หรือ 11 แบบ x64
2. [Rust](https://rustup.rs/) 1.90 ขึ้นไปที่ใช้ toolchain แบบ MSVC (`rustup default stable-x86_64-pc-windows-msvc`)
3. Visual Studio Build Tools ที่ติดตั้ง *Desktop development with C++* (ซึ่งมี Windows SDK มาด้วย)
4. สำหรับ `scripts\check.ps1` ให้ติดตั้ง cargo-deny ด้วย `cargo install cargo-deny`

```powershell
git clone https://github.com/ball46/duet.git
cd duet
cargo run --release -p duet-app          # เปิดตัวโปรแกรม
cargo run --release -p duet-cli -- a b   # ใช้ผ่าน command line
```

ถ้าเครื่องมีหน่วยความจำ 16 GB หรือน้อยกว่า ให้จำกัดจำนวนงานที่ทำพร้อมกัน เพื่อไม่ให้หน่วยความจำเต็มระหว่าง build และ test
`$env:CARGO_BUILD_JOBS = 2; $env:RUST_TEST_THREADS = 2`

### ก่อนส่ง pull request

`powershell -File scripts\check.ps1` ต้องผ่านทั้งหมด สคริปต์นี้รัน

- `cargo fmt --all -- --check` ตรวจรูปแบบโค้ด
- `cargo clippy --workspace --all-targets -- -D warnings` ตรวจ lint
- `cargo test --workspace` รัน test ราว 1,750 ข้อ (test หน้าจอจะขับหน้าจอจริงโดยไม่เปิดหน้าต่าง)
- `cargo deny check bans licenses sources` ตรวจ dependency และสัญญาอนุญาต

ความเร็วสำคัญมากใน duet ถ้าแก้ส่วนที่เทียบข้อความ สแกนโฟลเดอร์ หรือการเลื่อนหน้าจอ ให้รัน `scripts\bench.ps1`
ทั้งก่อนและหลังแก้ แล้วใส่ตัวเลขทั้งสองค่าไว้ใน pull request

### กติกา

- **ออฟไลน์เสมอ** duet ไม่ติดต่อเครือข่ายเลย ห้ามเพิ่ม crate ที่ใช้ HTTP หรือส่งข้อมูลการใช้งาน `deny.toml` ห้ามไว้ และมี test ตรวจว่าโปรแกรมไม่ได้ตั้งค่า HTTP client
- **แก้พร้อม test** แก้บั๊กต้องมี test ที่ไม่ผ่านถ้าไม่มีการแก้นั้น และพฤติกรรมใหม่ต้องมี test
- **pull request เล็กและตรงเรื่อง** หนึ่งอันทำเรื่องเดียว และไม่จัดรูปแบบโค้ดส่วนที่ไม่เกี่ยวข้อง
- **ไฟล์ซอร์สเป็น UTF-8** แก้ด้วย editor หรือสคริปต์ใน `scripts\` อย่าใช้ `Set-Content` ของ Windows PowerShell 5.1 เพราะจะเขียนไฟล์ UTF-8 เป็น code page อื่น (`crates/duet-app/tests/source_hygiene.rs` จะจับได้)
- **ข้อความบนหน้าจอ** ให้สั้นและเข้าใจง่าย ถ้าแก้ป้ายชื่อปุ่ม ให้แก้ในคู่มือด้วย ทั้ง `crates/duet-app/src/help/en.rs` และ `th.rs`
- **ข้อความ commit** ใช้รูปแบบ [Conventional Commits](https://www.conventionalcommits.org/) เช่น `feat:`, `fix:`, `docs:`, `refactor:`, `test:`, `perf:`, `chore:`

ดูว่าโค้ดส่วนไหนอยู่ที่ไหนได้จากตาราง [Where things are](#where-things-are) ด้านบน ส่วนคำว่า **LocalDiff**
ที่เจอใน comment คือโปรแกรมเทียบไฟล์ตัวก่อนหน้าของผู้สร้าง ซึ่งเขียนด้วย Electron duet ทำขึ้นมาแทน จึงใช้ฟีเจอร์และหน้าตาของ LocalDiff เป็นเกณฑ์

### สัญญาอนุญาต

งานที่ส่งเข้ามาจะเผยแพร่ภายใต้สัญญาอนุญาต [MIT](LICENSE) เหมือนกับตัวโปรเจกต์
