"""Writes the licence notices of every crate built into duet.exe and duet-cli.exe.

    python scripts/third-party-notices.py [out.md]      (default: dist/THIRD_PARTY_NOTICES.md)

Walks `cargo metadata` from duet-app and duet-cli through normal (not dev or build) dependencies
for 64-bit Windows, and copies each crate's own licence and notice files, so every copyright line
travels with the binaries. Identical texts are printed once, with the crates that share them.
"""
import hashlib
import json
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
TARGET = "x86_64-pc-windows-msvc"
ROOTS = {"duet-app", "duet-cli"}
LICENCE_PREFIXES = ("license", "licence", "copying", "notice", "copyright", "unlicense")


def metadata():
    out = subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--offline", "--filter-platform", TARGET],
        cwd=ROOT, check=True, capture_output=True, text=True, encoding="utf-8",
    ).stdout
    return json.loads(out)


def shipped(meta):
    """Package ids reachable from the two binaries through normal dependencies."""
    nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}
    names = {p["id"]: p["name"] for p in meta["packages"]}
    todo = [pid for pid, name in names.items() if name in ROOTS]
    seen = set()
    while todo:
        pid = todo.pop()
        if pid in seen:
            continue
        seen.add(pid)
        for dep in nodes[pid]["deps"]:
            if any(k["kind"] is None for k in dep["dep_kinds"]):
                todo.append(dep["pkg"])
    return seen


def licence_files(manifest):
    folder = Path(manifest).parent
    files = []
    for f in sorted(folder.iterdir()):
        if f.is_file() and f.name.lower().startswith(LICENCE_PREFIXES):
            files.append(f)
    for sub in ("LICENSES", "licenses", "license"):
        d = folder / sub
        if d.is_dir():
            files += sorted(p for p in d.iterdir() if p.is_file())
    return files


def main():
    out = Path(sys.argv[1]) if len(sys.argv) > 1 else ROOT / "dist" / "THIRD_PARTY_NOTICES.md"
    meta = metadata()
    ids = shipped(meta)
    packages = sorted((p for p in meta["packages"] if p["id"] in ids and p["source"]),
                      key=lambda p: (p["name"], p["version"]))
    texts = {}       # hash -> text
    users = {}       # hash -> [crate names]
    missing = []
    rows = []
    for p in packages:
        label = f"{p['name']} {p['version']}"
        authors = ", ".join(x.split("<")[0].strip() for x in p.get("authors") or []) or "its authors"
        rows.append((label, p.get("license") or "see files", p.get("repository") or "", authors))
        files = licence_files(p["manifest_path"])
        if not files:
            missing.append(label)
        for f in files:
            text = f.read_text(encoding="utf-8", errors="replace").strip()
            h = hashlib.sha256(text.encode()).hexdigest()
            texts.setdefault(h, text)
            users.setdefault(h, []).append(f"{label} ({f.name})")
    lines = [
        "# Third-party notices",
        "",
        "duet is MIT-licensed (see LICENSE). It is built with the open-source crates listed below,",
        "each under its own licence. Their licence and notice files follow, exactly as each crate ships them.",
        "",
        f"{len(packages)} crates.",
        "",
        "| Crate | Licence | Copyright holders | Source |",
        "|---|---|---|---|",
    ]
    lines += [f"| {n} | {l} | {a} | {r} |" for n, l, r, a in rows]
    if missing:
        lines += ["", "These crates ship no licence file. Each is under the licence named in the table above,",
                  "copyright its listed authors; the full MIT and Apache-2.0 texts appear below with other crates.", ""]
        lines += [f"- {m}" for m in missing]
    lines += ["", "## Licence texts", ""]
    for h, text in texts.items():
        lines += ["---", "", "Used by: " + ", ".join(users[h]), "", "```text", text.replace("```", "'''"), "```", ""]
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")
    print(f"{out}: {len(packages)} crates, {len(texts)} distinct texts, {len(missing)} without a file")


if __name__ == "__main__":
    main()
