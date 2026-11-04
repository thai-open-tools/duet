//! The HTML report of a folder compare (R6, `rep:403-650`): one self-contained, offline page with
//! inline CSS and one inline script. Everything that comes from a file, a path or a name is
//! escaped; the line cells come from the text tab's `html_line`, so both reports mark characters,
//! tabs and invisible characters alike.

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;

use chrono::NaiveDateTime;

use crate::folder::report::lines::{DetailRow, LineDetails, LineInfo, RowClass, line_summary};
use crate::folder::report::{
    Attention, ItemState, NOT_SCANNED_HINT, ReportContext, ReportCounts, ReportItem, ReportOptions,
    attention, count_text, criterion_text, human_bytes, percent, report_counts, top_groups,
    unimportant_reason,
};
use crate::folder::run::CompareMode;
use crate::text::report::html_line;

#[cfg(test)]
mod tests;

/// `RPT_CSS` (`rep:262-336`): LocalDiff's light palette and layout, the same selectors.
const CSS: &str = concat!(
    ":root{--ink:#1f2329;--ink2:#4c5561;--muted:#7a8391;--line:#e2e6ec;--page:#f5f6f8;--card:#fff;",
    "--a:#c23d52;--d:#a5770f;--b:#1e8a5c;--s:#b8bec7;--ta:#fbe9ec;--td:#fbf3df;--tb:#e3f3ea;--ts:#eef0f3;--warn:#b0432b;--tw:#fdf0ea}",
    "*{box-sizing:border-box}html{background:var(--page)}body{margin:0;color:var(--ink);background:var(--page);font:14px/1.6 \"Segoe UI\",\"Leelawadee UI\",\"Noto Sans Thai\",Tahoma,sans-serif}",
    ".wrap{max-width:1180px;margin:0 auto;padding:28px 20px 64px}a{color:#2c5d9e;text-decoration:none}a:hover{text-decoration:underline}",
    "code,.mono{font-family:Consolas,\"Cascadia Mono\",monospace;font-size:12.5px}",
    ".brand{color:var(--muted);font-size:12.5px;display:flex;justify-content:space-between;gap:12px;flex-wrap:wrap}",
    "h1{font-size:23px;line-height:1.35;margin:6px 0 4px}h2{font-size:17px;margin:34px 0 12px;padding-bottom:6px;border-bottom:1px solid var(--line)}",
    "h2 small{font-weight:400;color:var(--muted);font-size:13px;margin-left:6px}.note{white-space:pre-wrap;color:var(--ink2);margin:6px 0 0}",
    ".sides{display:grid;gap:8px;margin:16px 0 10px}.side{display:grid;grid-template-columns:auto auto 1fr auto;gap:4px 12px;align-items:baseline;background:var(--card);border:1px solid var(--line);border-radius:8px;padding:9px 14px}",
    ".tag{display:inline-block;min-width:24px;text-align:center;font-weight:700;border-radius:5px;background:var(--ink);color:#fff;font-size:12px;padding:1px 6px}",
    ".side b{font-size:15px}.side code{color:var(--muted);word-break:break-all}.side .m{color:var(--muted);font-size:12.5px;white-space:nowrap}",
    ".cfg{color:var(--muted);font-size:12.5px}.cfg b{color:var(--ink2);font-weight:600}",
    ".kpis{display:grid;grid-template-columns:repeat(auto-fit,minmax(150px,1fr));gap:10px}.kpi{background:var(--card);border:1px solid var(--line);border-radius:10px;padding:12px 14px}",
    ".kpi .l{color:var(--ink2);font-size:13px;display:flex;align-items:center;gap:7px}.kpi .v{font-size:30px;font-weight:650;line-height:1.2;margin-top:4px;font-variant-numeric:tabular-nums}",
    ".kpi .p{color:var(--muted);font-size:12.5px}.sw{display:inline-block;width:10px;height:10px;border-radius:3px;flex:0 0 auto}",
    ".sw.a{background:var(--a)}.sw.d{background:var(--d)}.sw.b{background:var(--b)}.sw.s{background:var(--s)}",
    ".bar{display:flex;gap:2px;height:18px;margin:16px 0 8px}.bar span{display:block;min-width:3px;height:100%}.bar span:first-child{border-radius:4px 0 0 4px}.bar span:last-child{border-radius:0 4px 4px 0}",
    ".bar span:only-child{border-radius:4px}.bar .a{background:var(--a)}.bar .d{background:var(--d)}.bar .b{background:var(--b)}.bar .s{background:var(--s)}",
    ".legend{display:flex;flex-wrap:wrap;gap:6px 18px;color:var(--ink2);font-size:13px}.legend span{display:inline-flex;align-items:center;gap:6px}",
    ".findings{background:var(--card);border:1px solid var(--line);border-radius:10px;padding:10px 16px 10px 34px;margin:16px 0 0}.findings li{margin:3px 0}",
    ".att{background:var(--card);border:1px solid var(--line);border-left:4px solid var(--warn);border-radius:8px;padding:10px 14px;margin:0 0 10px}",
    ".att h3{margin:0;font-size:15px;color:var(--warn)}.att p{margin:2px 0 6px;color:var(--ink2);font-size:13px}.att ul{margin:0;padding-left:18px;columns:2 360px;column-gap:28px}.att li{font-size:13px;break-inside:avoid}",
    "table.grp{width:100%;border-collapse:collapse;background:var(--card);border:1px solid var(--line);border-radius:8px;overflow:hidden;font-variant-numeric:tabular-nums}",
    "table.grp th,table.grp td{padding:6px 12px;border-bottom:1px solid var(--line);text-align:right;white-space:nowrap}table.grp th{background:#f8f9fb;color:var(--ink2);font-weight:600;font-size:12.5px}",
    "table.grp td:first-child,table.grp th:first-child{text-align:left;white-space:normal;word-break:break-all}table.grp td.z{color:#c3c8cf}table.grp td.mb{width:180px}",
    ".mbar{display:flex;gap:2px;height:9px;width:170px}.mbar span{height:100%;min-width:2px}.mbar span:first-child{border-radius:3px 0 0 3px}.mbar span:last-child{border-radius:0 3px 3px 0}.mbar span:only-child{border-radius:3px}",
    ".mbar .a{background:var(--a)}.mbar .d{background:var(--d)}.mbar .b{background:var(--b)}.mbar .s{background:var(--s)}",
    ".tools{position:sticky;top:0;z-index:5;display:flex;flex-wrap:wrap;align-items:center;gap:8px 14px;background:var(--page);padding:10px 0;border-bottom:1px solid var(--line);margin-bottom:12px}",
    ".tools input[type=search]{font:inherit;padding:5px 10px;border:1px solid #c9cfd8;border-radius:6px;min-width:240px;background:#fff}",
    ".tools label{display:inline-flex;align-items:center;gap:5px;font-size:13px;color:var(--ink2);cursor:pointer}",
    ".tools button{font:inherit;font-size:13px;padding:4px 10px;border:1px solid #c9cfd8;border-radius:6px;background:#fff;cursor:pointer}.tools .cnt{color:var(--muted);font-size:12.5px;margin-left:auto}",
    "h3.sub{font-size:15px;margin:22px 0 10px;display:flex;align-items:center;gap:8px;flex-wrap:wrap}h3.sub small{color:var(--muted);font-weight:400;font-size:12.5px}",
    ".dgrp{margin:0 0 16px}.dgrp h4{font-size:13.5px;margin:0 0 6px;display:flex;flex-wrap:wrap;gap:4px 10px;align-items:baseline}.dgrp h4 small{color:var(--muted);font-weight:400}",
    ".og{background:var(--card);border:1px solid var(--line);border-radius:8px;margin:0 0 8px;padding:7px 12px}.oh{display:flex;flex-wrap:wrap;align-items:center;gap:4px 10px}.oh .mono{font-weight:600}",
    ".og details>summary{cursor:pointer;color:#2c5d9e;font-size:12.5px;margin-top:4px}",
    "ul.fl{list-style:none;margin:6px 0 2px;padding:0 0 0 22px;columns:3 250px;column-gap:24px}ul.fl li{font:12.5px/1.7 Consolas,\"Cascadia Mono\",monospace;break-inside:avoid;word-break:break-all}",
    "ul.fl li .sz{font:11px \"Segoe UI\",sans-serif;color:var(--muted);white-space:nowrap}ul.fl li:target{background:#fff4c4}.f:target{outline:2px solid #e5b72d}",
    ".f{background:var(--card);border:1px solid var(--line);border-radius:8px;margin:0 0 6px}.f>summary,.f>.row{list-style:none;display:flex;flex-wrap:wrap;align-items:center;gap:4px 10px;padding:7px 12px;cursor:pointer}",
    ".f>.row{cursor:default}.f>summary::-webkit-details-marker{display:none}.f>summary::before{content:\"▸\";color:var(--muted);width:10px}.f[open]>summary::before{content:\"▾\"}.f>.row::before{content:\"\";width:10px}",
    ".f .nm{font-family:Consolas,\"Cascadia Mono\",monospace;font-size:13px;font-weight:600;word-break:break-all}.f .info{color:var(--ink2);font-size:12.5px}.f .when{color:var(--muted);font-size:12.5px;margin-left:auto}",
    ".pill{display:inline-flex;align-items:center;gap:5px;border-radius:999px;padding:0 9px;font-size:12px;font-weight:600;color:var(--ink);white-space:nowrap}",
    ".pill i{width:8px;height:8px;border-radius:2px;display:inline-block}.pill.diff{background:var(--td)}.pill.diff i{background:var(--d)}.pill.left{background:var(--ta)}.pill.left i{background:var(--a)}",
    ".pill.right{background:var(--tb)}.pill.right i{background:var(--b)}.pill.same{background:var(--ts)}.pill.same i{background:var(--s)}.pill.w{background:var(--tw);color:var(--warn)}",
    ".fd{padding:2px 12px 12px 34px}.fd .meta{color:var(--muted);font-size:12.5px;margin:0 0 6px}.fd .msg{background:#f8f9fb;border-radius:6px;padding:8px 12px;color:var(--ink2);font-size:13px}",
    "table.dt{border-collapse:collapse;width:100%;table-layout:fixed;font:12px/1.55 Consolas,\"Cascadia Mono\",monospace;border:1px solid var(--line)}",
    "table.dt th{font:600 12px \"Segoe UI\",\"Leelawadee UI\",sans-serif;text-align:left;padding:4px 8px;background:#f8f9fb;border-bottom:1px solid var(--line)}",
    "table.dt td{vertical-align:top;padding:0 8px;white-space:pre-wrap;word-break:break-all}td.ln{text-align:right;color:#98a1ae;background:#f8f9fb;border-right:1px solid var(--line)}",
    "td.l{border-right:2px solid #d5dbe4}tr.mod td.l,tr.mod td.r{background:#fbf3df}tr.del td.l{background:#fae6e9}tr.ins td.r{background:#e3f3ea}",
    "tr.soft td.l,tr.soft td.r,tr.udel td.l,tr.uins td.r{background:#f1effa}td.ph{background:repeating-linear-gradient(45deg,#fff 0 6px,#f3f4f7 6px 12px)}",
    "tr.mod td.l .ch,tr.del td.l .ch{background:#f2b8c1}tr.mod td.r .ch,tr.ins td.r .ch{background:#b7e3c9}tr.soft .ch{background:#ddd8f3}.ch{border-radius:2px}",
    "tr.skip td{text-align:center;color:#98a1ae;background:#f8f9fb;font:12px \"Segoe UI\",\"Leelawadee UI\",sans-serif;padding:2px}tr.more td{color:var(--ink2)}",
    ".odd{border-radius:2px;padding:0 1px;font-size:10px;font-family:\"Segoe UI\",sans-serif}.odd.o-space{color:#c23d52;background:rgba(194,61,82,.12)}",
    ".odd.o-zero,.odd.o-ctrl{color:#c23d52;border:1px solid #c23d52;background:rgba(194,61,82,.08)}.odd.o-lead{color:#98a1ae;background:transparent;border-color:#c8cdd4}",
    ".wsc{color:#b8bec7}details.sec>summary{cursor:pointer;font-weight:600;padding:6px 0}details.sec ul{margin:4px 0 0;padding-left:20px;columns:2 380px;column-gap:28px}details.sec li{font-size:13px;break-inside:avoid}",
    "dl.gl{display:grid;grid-template-columns:max-content 1fr;gap:6px 16px;background:var(--card);border:1px solid var(--line);border-radius:10px;padding:12px 16px;font-size:13px}dl.gl dt{font-weight:600}dl.gl dd{margin:0;color:var(--ink2)}",
    "h2,h3,.box,.f,ul.fl li,[id^=g-]{scroll-margin-top:72px}",
    "footer{margin-top:36px;color:var(--muted);font-size:12px;text-align:center}[hidden]{display:none!important}",
    "@media (max-width:640px){.side{grid-template-columns:auto 1fr}.side code,.side .m{grid-column:1/-1}.f .when{margin-left:0}}",
    "@media print{html,body{background:#fff}.wrap{max-width:none;padding:0}.tools{display:none}.f,.kpi,.att,.side{break-inside:avoid}h2{break-after:avoid}",
    ".f{border-color:#ccc}*{-webkit-print-color-adjust:exact;print-color-adjust:exact}}",
);

/// `RPT_JS` (`rep:338-353`): search, the state filters, expand and collapse all, print. ES5.
const JS: &str = concat!(
    "(function(){",
    "var q=document.getElementById(\"q\"),bx=[].slice.call(document.querySelectorAll(\".tools input[type=checkbox]\")),cnt=document.getElementById(\"cnt\");",
    "function apply(){var t=(q.value||\"\").trim().toLowerCase(),on={},n=0;bx.forEach(function(b){on[b.value]=b.checked;});",
    "[].forEach.call(document.querySelectorAll(\".it\"),function(el){var ok=on[el.getAttribute(\"data-st\")]!==false&&(!t||el.getAttribute(\"data-p\").indexOf(t)>=0);el.hidden=!ok;if(ok)n++;});",
    "[].forEach.call(document.querySelectorAll(\".box\"),function(g){g.hidden=!g.querySelector(\".it:not([hidden])\");});",
    "[].forEach.call(document.querySelectorAll(\"h3.sub\"),function(h){var e=h.nextElementSibling,v=false;while(e&&!/^H[23]$/.test(e.tagName)){if(!e.hidden&&e.classList.contains(\"box\"))v=true;e=e.nextElementSibling;}h.hidden=!v;});",
    "if(cnt)cnt.textContent=\"Showing \"+n.toLocaleString()+(n===1?\" file\":\" files\");}",
    "if(q){q.addEventListener(\"input\",apply);bx.forEach(function(b){b.addEventListener(\"change\",apply);});apply();}",
    "function all(v){[].forEach.call(document.querySelectorAll(\"details.f\"),function(d){if(!d.hidden)d.open=v;});}",
    "var x=document.getElementById(\"exp\"),y=document.getElementById(\"col\");if(x)x.onclick=function(){all(true);};if(y)y.onclick=function(){all(false);};",
    "var was=[];window.addEventListener(\"beforeprint\",function(){was=[].filter.call(document.querySelectorAll(\"details\"),function(d){return !d.open;});was.forEach(function(d){d.open=true;});});",
    "window.addEventListener(\"afterprint\",function(){was.forEach(function(d){d.open=false;});was=[];});",
    "if(location.hash){var e=document.getElementById(location.hash.slice(1));if(e&&e.tagName===\"DETAILS\")e.open=true;}",
    "document.addEventListener(\"click\",function(ev){var a=ev.target.closest&&ev.target.closest(\"a[href^='#f']\");if(!a)return;var e=document.getElementById(a.getAttribute(\"href\").slice(1));",
    "if(e){if(e.hidden){q.value=\"\";bx.forEach(function(b){b.checked=true;});apply();}for(var p=e;p;p=p.parentElement)if(p.tagName===\"DETAILS\")p.open=true;}});",
    "})();",
);

/// Rows shown for one attention kind, and names listed for clean folders.
const ATTENTION_LINKS: usize = 60;
const CLEAN_FOLDERS: usize = 40;

fn esc(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 8);
    push_esc(&mut out, text);
    out
}

fn push_esc(out: &mut String, text: &str) {
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(ch),
        }
    }
}

/// JavaScript's `encodeURIComponent`, so the `g-` anchors of folder names are valid fragments.
fn encode_component(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&byte) {
            out.push(char::from(byte));
        } else {
            let _ = write!(out, "%{byte:02X}");
        }
    }
    out
}

fn num(n: usize) -> String {
    count_text(n as u64, "", "").trim_end().to_owned()
}

fn files(n: usize) -> String {
    count_text(n as u64, "file", "files")
}

fn date_text(time: Option<NaiveDateTime>) -> String {
    time.map_or_else(
        || "unknown".to_owned(),
        |time| time.format("%Y-%m-%d %H:%M").to_string(),
    )
}

fn dir_of(rel: &str) -> &str {
    rel.rfind('/').map_or("", |at| &rel[..at])
}

fn base_of(rel: &str) -> &str {
    rel.rsplit('/').next().unwrap_or(rel)
}

fn dir_label(dir: &str) -> &str {
    if dir.is_empty() { "(top level)" } else { dir }
}

fn state_key(state: ItemState) -> &'static str {
    match state {
        ItemState::Different => "diff",
        ItemState::LeftOnly => "left",
        ItemState::RightOnly => "right",
        ItemState::Same => "same",
    }
}

fn state_rank(state: ItemState) -> u8 {
    match state {
        ItemState::Different => 0,
        ItemState::LeftOnly => 1,
        ItemState::RightOnly => 2,
        ItemState::Same => 3,
    }
}

fn state_label(state: ItemState, names: &[String; 2]) -> String {
    match state {
        ItemState::Different => "Different".to_owned(),
        ItemState::LeftOnly => format!("Only in {}", names[0]),
        ItemState::RightOnly => format!("Only in {}", names[1]),
        ItemState::Same => "Same".to_owned(),
    }
}

fn state_symbol(state: ItemState) -> &'static str {
    match state {
        ItemState::Different => "≠",
        ItemState::LeftOnly => "◀",
        ItemState::RightOnly => "▶",
        ItemState::Same => "=",
    }
}

fn pill(state: ItemState, names: &[String; 2]) -> String {
    format!(
        "<span class=\"pill {}\"><i></i>{} {}</span>",
        state_key(state),
        state_symbol(state),
        esc(&state_label(state, names))
    )
}

fn row_class(class: RowClass) -> &'static str {
    match class {
        RowClass::Same => "same",
        RowClass::Soft => "soft",
        RowClass::Mod => "mod",
        RowClass::Del => "del",
        RowClass::Ins => "ins",
        RowClass::UDel => "udel",
        RowClass::UIns => "uins",
    }
}

/// `rptBar`: the stacked bar of left only, different, right only and same.
fn bar(class: &str, left: usize, different: usize, right: usize, same: usize) -> String {
    let total = left + different + right + same;
    if total == 0 {
        return String::new();
    }
    let mut out = format!("<div class=\"{class}\">");
    for (css, n) in [("a", left), ("d", different), ("b", right), ("s", same)] {
        if n > 0 {
            let _ = write!(
                out,
                "<span class=\"{css}\" style=\"flex:{n} 1 0\" title=\"{} ({})\"></span>",
                files(n),
                percent(n, total)
            );
        }
    }
    out.push_str("</div>");
    out
}

fn line_cell(
    out: &mut String,
    side: Option<&(u32, String)>,
    class: char,
    marks: &[std::ops::Range<usize>],
    tab: usize,
) {
    match side {
        Some((number, text)) => {
            let _ = write!(
                out,
                "<td class=\"ln\">{number}</td><td class=\"{class}\">{}</td>",
                html_line(text, marks, tab, true)
            );
        }
        None => out.push_str("<td class=\"ln\"></td><td class=\"ph\"></td>"),
    }
}

fn detail_table(out: &mut String, rows: &[DetailRow], names: &[String; 2], tab: usize) {
    let _ = write!(
        out,
        "<table class=\"dt\"><colgroup><col style=\"width:52px\"><col><col style=\"width:52px\"><col></colgroup><tr><th colspan=\"2\">A · {}</th><th colspan=\"2\">B · {}</th></tr>",
        esc(&names[0]),
        esc(&names[1])
    );
    for row in rows {
        match row {
            DetailRow::Line {
                class,
                left,
                right,
                marks,
            } => {
                let (ml, mr): (&[_], &[_]) = marks
                    .as_ref()
                    .map_or((&[], &[]), |(l, r)| (l.as_slice(), r.as_slice()));
                let _ = write!(out, "<tr class=\"{}\">", row_class(*class));
                line_cell(out, left.as_ref(), 'l', ml, tab);
                line_cell(out, right.as_ref(), 'r', mr, tab);
                out.push_str("</tr>");
            }
            DetailRow::Skip(n) => {
                let _ = write!(
                    out,
                    "<tr class=\"skip\"><td colspan=\"4\">⋯ {} ⋯</td></tr>",
                    count_text(*n as u64, "same line", "same lines")
                );
            }
            DetailRow::More(n) => {
                let _ = write!(
                    out,
                    "<tr class=\"skip more\"><td colspan=\"4\">… {}: open this file in duet to see them all</td></tr>",
                    count_text(*n as u64, "more row", "more rows")
                );
            }
        }
    }
    out.push_str("</table>");
}

struct Page<'a> {
    options: &'a ReportOptions,
    context: &'a ReportContext,
    lines: Option<&'a LineDetails>,
    /// The listed files, in the order their ids follow.
    ids: HashMap<&'a str, usize>,
    anchored: HashSet<String>,
    groups: Vec<crate::folder::report::Group>,
}

impl<'a> Page<'a> {
    fn names(&self) -> &'a [String; 2] {
        &self.options.names
    }

    fn info(&self, item: &ReportItem) -> Option<&'a LineInfo> {
        self.lines.and_then(|lines| lines.files.get(&item.rel))
    }

    fn link(&self, item: &ReportItem) -> String {
        match self.ids.get(item.rel.as_str()) {
            Some(id) => format!("<a class=\"mono\" href=\"#f{id}\">{}</a>", esc(&item.rel)),
            None => format!("<span class=\"mono\">{}</span>", esc(&item.rel)),
        }
    }

    /// The `g-` anchors for the groups that start at directory `dir` (once each).
    fn anchors_for(&mut self, dir: &str) -> String {
        let mut out = String::new();
        for group in &self.groups {
            let folder = &group.folder;
            if !folder.is_empty()
                && !self.anchored.contains(folder)
                && (dir == folder || dir.starts_with(&format!("{folder}/")))
            {
                self.anchored.insert(folder.clone());
                let _ = write!(
                    out,
                    "<span id=\"g-{}\"></span>",
                    esc(&encode_component(folder))
                );
            }
        }
        out
    }

    /// The message a differing file shows when it has no line table.
    fn message(&self, item: &ReportItem, info: Option<&LineInfo>) -> String {
        let names = self.names();
        let formats = item.note.as_ref().and_then(|note| note.formats);
        let mut msg = if let Some(note) = info.and_then(|info| info.note.as_ref()) {
            note.clone()
        } else if item.error.is_some() {
            "Could not read the file: the contents were not compared.".to_owned()
        } else if item.case_only() {
            let spell = |side: &Option<crate::folder::report::SideFacts>| {
                side.as_ref().map(|s| s.rel.clone()).unwrap_or_default()
            };
            format!(
                "Same contents; the names differ only in case: {} / {}",
                spell(&item.left),
                spell(&item.right)
            )
        } else if item.format_only()
            && let Some([a, b]) = formats
        {
            format!(
                "Every line matches; only the file format differs — {}: {} · {}: {}",
                names[0],
                a.describe(),
                names[1],
                b.describe()
            )
        } else if !self.context.reads_contents {
            if self.context.criterion == criterion_text(CompareMode::Size) {
                "Compared by size (contents not read)".to_owned()
            } else {
                "Compared by time and size (contents not read)".to_owned()
            }
        } else if self.options.details {
            String::new()
        } else {
            "Differing lines were not requested".to_owned()
        };
        if !msg.is_empty()
            && item.note.as_ref().is_some_and(|note| note.soft)
            && let Some([a, b]) = formats
        {
            let _ = write!(
                msg,
                " · {}: {} / {}: {}",
                names[0],
                a.describe(),
                names[1],
                b.describe()
            );
        }
        msg
    }

    /// `rptFileCard` (`rep:565-609`).
    fn file_card(&self, item: &ReportItem, id: usize) -> String {
        let names = self.names();
        let info = self.info(item);
        let mut head = pill(item.state, names);
        let _ = write!(
            head,
            "<span class=\"nm\">{}</span>",
            esc(base_of(&item.rel))
        );
        if let Some(info) = info {
            let summary = line_summary(info, names);
            if !summary.is_empty() {
                let _ = write!(head, "<span class=\"info\">{}</span>", esc(&summary));
            }
        }
        for (kind, _) in attention(item, names) {
            let _ = write!(
                head,
                "<span class=\"pill w\">⚠ {}</span>",
                esc(kind.title())
            );
        }
        if item.state == ItemState::Different
            && let Some(side) = item.newer
        {
            let name = match side {
                crate::folder::tree::Side::Left => &names[0],
                crate::folder::tree::Side::Right => &names[1],
            };
            let _ = write!(
                head,
                "<span class=\"when\">{} was modified later</span>",
                esc(name)
            );
        }
        let facts = |side: &Option<crate::folder::report::SideFacts>, name: &str| match side {
            Some(s) => format!(
                "{}: {} · modified {}",
                esc(name),
                human_bytes(s.len),
                date_text(s.modified)
            ),
            None => format!("Not in {}", esc(name)),
        };
        let data_p = esc(&item.rel.to_lowercase());
        let key = state_key(item.state);
        let mut body = String::new();
        if item.state == ItemState::Different {
            match info {
                Some(info) if !info.rows.is_empty() => {
                    detail_table(&mut body, &info.rows, names, self.context.tab_width);
                }
                _ => {
                    let msg = self.message(item, info);
                    if !msg.is_empty() {
                        let _ = write!(body, "<div class=\"msg\">{}</div>", esc(&msg));
                    }
                }
            }
        }
        if !body.is_empty() {
            return format!(
                "<details class=\"f it\" id=\"f{id}\" data-st=\"{key}\" data-p=\"{data_p}\"><summary>{head}</summary><div class=\"fd\"><div class=\"meta\">{} &nbsp;|&nbsp; {}</div>{body}</div></details>",
                facts(&item.left, &names[0]),
                facts(&item.right, &names[1]),
            );
        }
        // One-sided and same files: one row, nothing to open.
        let side = match item.state {
            ItemState::LeftOnly => item.left.as_ref().map(|s| {
                format!(
                    "{} · modified {}",
                    human_bytes(s.len),
                    date_text(s.modified)
                )
            }),
            ItemState::RightOnly => item.right.as_ref().map(|s| {
                format!(
                    "{} · modified {}",
                    human_bytes(s.len),
                    date_text(s.modified)
                )
            }),
            _ => item.left.as_ref().map(|s| human_bytes(s.len)),
        }
        .unwrap_or_default();
        format!(
            "<div class=\"f it\" id=\"f{id}\" data-st=\"{key}\" data-p=\"{data_p}\"><div class=\"row\">{head}<span class=\"when\">{}</span></div></div>",
            esc(&side)
        )
    }

    /// One-sided files: whole missing folders on one line each, the rest by folder.
    fn one_sided(&mut self, out: &mut String, list: &[&'a ReportItem], state: ItemState) {
        if list.is_empty() {
            return;
        }
        let names = self.names();
        let (mine, other, css) = if state == ItemState::LeftOnly {
            (&names[0], &names[1], "a")
        } else {
            (&names[1], &names[0], "b")
        };
        let _ = write!(
            out,
            "<h3 class=\"sub\"><span class=\"sw {css}\"></span>{} Only in {} (not in {}) — {}</h3>",
            state_symbol(state),
            esc(mine),
            esc(other),
            files(list.len())
        );
        let mut whole: HashMap<&str, Vec<&ReportItem>> = HashMap::new();
        let mut part: HashMap<&str, Vec<&ReportItem>> = HashMap::new();
        for &item in list {
            match item.whole_folder.as_deref() {
                Some(folder) => whole.entry(folder).or_default().push(item),
                None => part.entry(dir_of(&item.rel)).or_default().push(item),
            }
        }
        let size_of = |item: &ReportItem| {
            let side = if state == ItemState::LeftOnly {
                &item.left
            } else {
                &item.right
            };
            side.as_ref().map_or((0, None), |s| (s.len, s.modified))
        };
        let li = |item: &ReportItem, cut: Option<&str>, ids: &HashMap<&str, usize>| {
            let (len, modified) = size_of(item);
            let name = match cut {
                Some(dir) => item.rel.get(dir.len() + 1..).unwrap_or(&item.rel),
                None => base_of(&item.rel),
            };
            format!(
                "<li class=\"it\" id=\"f{}\" data-st=\"{}\" data-p=\"{}\" title=\"{}\">{} <span class=\"sz\">{}</span></li>",
                ids.get(item.rel.as_str()).copied().unwrap_or_default(),
                state_key(state),
                esc(&item.rel.to_lowercase()),
                esc(&format!("{} · modified {}", item.rel, date_text(modified))),
                esc(name),
                human_bytes(len)
            )
        };
        for dir in sorted_keys(&whole) {
            let group = &whole[dir];
            let total: u64 = group.iter().map(|item| size_of(item).0).sum();
            let _ = write!(
                out,
                "<div class=\"og box\">{}<div class=\"oh\"><span class=\"mono\">📁 {}/</span><span class=\"pill {}\"><i></i>The whole folder is missing in {}</span><span class=\"info\">{} · {}</span></div><details><summary>File list</summary><ul class=\"fl\">",
                self.anchors_for(dir),
                esc(dir),
                state_key(state),
                esc(other),
                files(group.len()),
                human_bytes(total)
            );
            for item in group {
                out.push_str(&li(item, Some(dir), &self.ids));
            }
            out.push_str("</ul></details></div>");
        }
        for dir in sorted_keys(&part) {
            let group = &part[dir];
            let _ = write!(
                out,
                "<div class=\"og box\">{}<div class=\"oh\"><span class=\"mono\">📁 {}</span><span class=\"info\">{}</span></div><ul class=\"fl\">",
                self.anchors_for(dir),
                esc(dir_label(dir)),
                files(group.len())
            );
            for item in group {
                out.push_str(&li(item, None, &self.ids));
            }
            out.push_str("</ul></div>");
        }
    }
}

/// The keys of a by-folder map, sorted by lower-cased name (`sortDirs`); items in each group
/// sorted by lower-cased path (`byName`).
fn sorted_keys<'a>(map: &HashMap<&'a str, Vec<&ReportItem>>) -> Vec<&'a str> {
    let mut keys: Vec<&str> = map.keys().copied().collect();
    keys.sort_by_cached_key(|key| (key.to_lowercase(), key.to_owned()));
    keys
}

/// Builds the report page. `items` may come in any order; the page sorts what it lists.
pub fn folder_html(
    items: &[ReportItem],
    lines: Option<&LineDetails>,
    options: &ReportOptions,
    context: &ReportContext,
) -> String {
    let names = &options.names;
    let counts = report_counts(items);
    let mut out = String::with_capacity(CSS.len() + JS.len() + 8192 + items.len() * 480);

    // What the Files section lists, and the ids the links use.
    let mut detail: Vec<&ReportItem> = items
        .iter()
        .filter(|item| item.state != ItemState::Same || (options.same && !item.unimportant()))
        .collect();
    detail.sort_by_cached_key(|item| (state_rank(item.state), item.rel.to_lowercase()));
    let ids: HashMap<&str, usize> = detail
        .iter()
        .enumerate()
        .map(|(id, item)| (item.rel.as_str(), id))
        .collect();
    let mut page = Page {
        options,
        context,
        lines,
        ids,
        anchored: HashSet::new(),
        groups: top_groups(items),
    };

    let title = if options.title.is_empty() {
        format!("{} compared with {}", names[0], names[1])
    } else {
        options.title.clone()
    };
    let _ = write!(
        out,
        "<!DOCTYPE html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>{} — folder compare report</title><style>{CSS}</style></head><body><div class=\"wrap\">",
        esc(&title)
    );
    let _ = write!(
        out,
        "<div class=\"brand\"><span>duet · folder compare report</span><span>{}</span></div><h1>{}</h1>",
        context.generated.format("%Y-%m-%d %H:%M"),
        esc(&title)
    );
    if !options.note.is_empty() {
        let _ = write!(out, "<p class=\"note\">{}</p>", esc(&options.note));
    }
    side_cards(&mut out, names, context, &counts);
    config_line(&mut out, options, context, lines);

    summary(&mut out, items, lines, options, &counts, &page);
    attention_section(&mut out, items, &page);
    by_folder(&mut out, &page, names);
    if !detail.is_empty() {
        files_section(&mut out, &mut page, &detail);
    }
    let unimportant: Vec<&ReportItem> = items.iter().filter(|item| item.unimportant()).collect();
    if options.unimportant && !unimportant.is_empty() {
        let _ = write!(
            out,
            "<h2>Differs only in unimportant parts<small>counted as Same · {}</small></h2><details class=\"sec\"><summary>Show the list</summary><ul>",
            files(unimportant.len())
        );
        for item in &unimportant {
            let _ = write!(
                out,
                "<li><span class=\"mono\">{}</span> — <span class=\"info\">{}</span></li>",
                esc(&item.rel),
                esc(&unimportant_reason(item).unwrap_or_default())
            );
        }
        out.push_str("</ul></details>");
    }
    legend(&mut out, options);
    let _ = write!(
        out,
        "<footer>Made with duet {} · works offline; no data leaves this computer</footer></div>",
        esc(&context.version)
    );
    if !detail.is_empty() {
        let _ = write!(out, "<script>{JS}</script>");
    }
    out.push_str("</body></html>");
    out
}

fn side_cards(out: &mut String, names: &[String; 2], context: &ReportContext, c: &ReportCounts) {
    out.push_str("<div class=\"sides\">");
    for (tag, name, path, n, bytes) in [
        (
            "A",
            &names[0],
            &context.side_paths[0],
            c.left_files,
            c.left_bytes,
        ),
        (
            "B",
            &names[1],
            &context.side_paths[1],
            c.right_files,
            c.right_bytes,
        ),
    ] {
        let _ = write!(
            out,
            "<div class=\"side\"><span class=\"tag\">{tag}</span><b>{}</b><code>{}</code><span class=\"m\">{} · {}</span></div>",
            esc(name),
            esc(path),
            files(n),
            human_bytes(bytes)
        );
    }
    out.push_str("</div>");
}

fn config_line(
    out: &mut String,
    options: &ReportOptions,
    context: &ReportContext,
    lines: Option<&LineDetails>,
) {
    let _ = write!(
        out,
        "<div class=\"cfg\">Criterion: <b>{}</b> · Scope: <b>{}</b>",
        esc(&context.criterion),
        esc(&context.scope)
    );
    if !context.include.is_empty() {
        let _ = write!(out, " · Include: <b>{}</b>", esc(&context.include));
    }
    if !context.exclude.is_empty() {
        let _ = write!(out, " · Exclude: <b>{}</b>", esc(&context.exclude));
    }
    if let (Some(_), Some(rules)) = (lines, &context.rules) {
        let _ = write!(out, " · Line compare: <b>{}</b>", esc(rules));
    }
    if let (Some(lines), true) = (lines, options.mask) {
        out.push_str(" · <b>Values that look like passwords or keys are hidden</b>");
        if lines.masked_lines > 0 {
            let _ = write!(
                out,
                " ({})",
                count_text(lines.masked_lines as u64, "line", "lines")
            );
        }
    }
    if !context.invisible_scan {
        let _ = write!(
            out,
            " · Invisible characters: <b>not scanned</b> ({NOT_SCANNED_HINT})"
        );
    }
    out.push_str("</div>");
}

fn summary(
    out: &mut String,
    items: &[ReportItem],
    lines: Option<&LineDetails>,
    options: &ReportOptions,
    c: &ReportCounts,
    page: &Page,
) {
    let names = &options.names;
    let mismatched = c.mismatched();
    let kpi = |swatch: &str, label: String, value: usize, note: String| {
        let sw = if swatch.is_empty() {
            String::new()
        } else {
            format!("<span class=\"sw {swatch}\"></span>")
        };
        format!(
            "<div class=\"kpi\"><div class=\"l\">{sw}{label}</div><div class=\"v\">{}</div><div class=\"p\">{note}</div></div>",
            num(value)
        )
    };
    out.push_str("<h2>Summary</h2><div class=\"kpis\">");
    out.push_str(&kpi(
        "",
        "All files".to_owned(),
        c.total,
        format!(
            "Mismatched {} ({})",
            num(mismatched),
            percent(mismatched, c.total)
        ),
    ));
    out.push_str(&kpi(
        "d",
        "Different".to_owned(),
        c.different,
        "On both sides, contents differ".to_owned(),
    ));
    out.push_str(&kpi(
        "a",
        format!("Only in {}", esc(&names[0])),
        c.left_only,
        format!("Not in {}", esc(&names[1])),
    ));
    out.push_str(&kpi(
        "b",
        format!("Only in {}", esc(&names[1])),
        c.right_only,
        format!("Not in {}", esc(&names[0])),
    ));
    out.push_str(&kpi(
        "s",
        "Same".to_owned(),
        c.same,
        if c.unimportant > 0 {
            format!(
                "Including {} that {} only in unimportant parts",
                num(c.unimportant),
                if c.unimportant == 1 {
                    "differs"
                } else {
                    "differ"
                }
            )
        } else {
            "Contents match".to_owned()
        },
    ));
    out.push_str("</div>");
    out.push_str(&bar("bar", c.left_only, c.different, c.right_only, c.same));
    let _ = write!(
        out,
        "<div class=\"legend\"><span><span class=\"sw a\"></span>◀ Only in {} {}</span><span><span class=\"sw d\"></span>≠ Different {}</span><span><span class=\"sw b\"></span>▶ Only in {} {}</span><span><span class=\"sw s\"></span>= Same {}</span></div>",
        esc(&names[0]),
        percent(c.left_only, c.total),
        percent(c.different, c.total),
        esc(&names[1]),
        percent(c.right_only, c.total),
        percent(c.same, c.total)
    );

    let mut findings: Vec<String> = Vec::new();
    if mismatched == 0 {
        findings.push("Every file matches: nothing to check.".to_owned());
    } else {
        findings.push(format!(
            "<b>{}</b> of {} files do not match: {} with different contents, {} only in {}, {} only in {}",
            num(mismatched),
            num(c.total),
            num(c.different),
            num(c.left_only),
            esc(&names[0]),
            num(c.right_only),
            esc(&names[1])
        ));
        let mut hot: Vec<&crate::folder::report::Group> = page
            .groups
            .iter()
            .filter(|g| g.different + g.left_only + g.right_only > 0)
            .collect();
        hot.sort_by_key(|g| std::cmp::Reverse(g.different + g.left_only + g.right_only));
        if hot.len() > 1 {
            let top: Vec<String> = hot
                .iter()
                .take(3)
                .map(|g| {
                    format!(
                        "<span class=\"mono\">{}</span> ({})",
                        esc(dir_label(&g.folder)),
                        num(g.different + g.left_only + g.right_only)
                    )
                })
                .collect();
            findings.push(format!(
                "Folders with the most differences: {}",
                top.join(", ")
            ));
        }
    }
    if let Some(lines) = lines {
        let (mut changed, mut left, mut right, mut read) = (0u64, 0u64, 0u64, 0usize);
        for item in items {
            if let Some(info) = lines.files.get(&item.rel).filter(|info| info.counted) {
                changed += u64::from(info.changed);
                left += u64::from(info.left_only);
                right += u64::from(info.right_only);
                read += 1;
            }
        }
        if read > 0 {
            findings.push(format!(
                "Differing lines in total ({} read): {} changed · {} only in {} · {} only in {}",
                count_text(read as u64, "file", "files"),
                num(changed as usize),
                num(left as usize),
                esc(&names[0]),
                num(right as usize),
                esc(&names[1])
            ));
        }
    }
    let attention_total = attention_groups(items, page)
        .iter()
        .map(Vec::len)
        .sum::<usize>();
    if attention_total > 0 {
        findings.push(format!(
            "⚠ <b>{}</b> {} a closer look: see the next section",
            num(attention_total),
            if attention_total == 1 {
                "item needs"
            } else {
                "items need"
            }
        ));
    }
    if c.unimportant > 0 {
        findings.push(format!(
            "{} {} only in the file format (EOL/BOM) or in unimportant parts: counted as Same",
            files(c.unimportant),
            if c.unimportant == 1 {
                "differs"
            } else {
                "differ"
            }
        ));
    }
    out.push_str("<ul class=\"findings\">");
    for finding in findings {
        let _ = write!(out, "<li>{finding}</li>");
    }
    out.push_str("</ul>");
}

/// Per attention kind (in `Attention::ALL` order), the items and their extra text.
fn attention_groups<'a>(
    items: &'a [ReportItem],
    page: &Page,
) -> Vec<Vec<(&'a ReportItem, Option<String>)>> {
    let mut groups: Vec<Vec<(&ReportItem, Option<String>)>> =
        vec![Vec::new(); Attention::ALL.len()];
    for item in items {
        for (kind, extra) in attention(item, page.names()) {
            if let Some(at) = Attention::ALL.iter().position(|k| *k == kind) {
                groups[at].push((item, extra));
            }
        }
    }
    groups
}

fn attention_section(out: &mut String, items: &[ReportItem], page: &Page) {
    let groups = attention_groups(items, page);
    let total: usize = groups.iter().map(Vec::len).sum();
    if total == 0 {
        return;
    }
    let _ = write!(
        out,
        "<h2>⚠ Needs a closer look<small>{}</small></h2>",
        count_text(total as u64, "item", "items")
    );
    for (kind, list) in Attention::ALL.iter().zip(&groups) {
        if list.is_empty() {
            continue;
        }
        let _ = write!(
            out,
            "<div class=\"att\"><h3>{} — {}</h3><p>{}</p><ul>",
            esc(kind.title()),
            files(list.len()),
            esc(kind.help())
        );
        for (item, extra) in list.iter().take(ATTENTION_LINKS) {
            let _ = write!(out, "<li>{}", page.link(item));
            if let Some(extra) = extra {
                let _ = write!(out, " <span class=\"info\">({})</span>", esc(extra));
            }
            out.push_str("</li>");
        }
        if list.len() > ATTENTION_LINKS {
            let _ = write!(
                out,
                "<li>…and {} more</li>",
                files(list.len() - ATTENTION_LINKS)
            );
        }
        out.push_str("</ul></div>");
    }
}

fn by_folder(out: &mut String, page: &Page, names: &[String; 2]) {
    if page.groups.len() <= 1 {
        return;
    }
    let mismatched = |g: &crate::folder::report::Group| g.different + g.left_only + g.right_only;
    let mut groups: Vec<&crate::folder::report::Group> = page.groups.iter().collect();
    groups.sort_by(|x, y| {
        mismatched(y)
            .cmp(&mismatched(x))
            .then_with(|| x.folder.cmp(&y.folder))
    });
    let (hot, clean): (Vec<_>, Vec<_>) = groups.into_iter().partition(|g| mismatched(g) > 0);
    let cell = |n: usize| {
        if n > 0 {
            format!("<td>{}</td>", num(n))
        } else {
            "<td class=\"z\">0</td>".to_owned()
        }
    };
    let _ = write!(
        out,
        "<h2>By folder<small>the most different first</small></h2><table class=\"grp\"><tr><th>Folder</th><th>≠ Different</th><th>◀ Only in {}</th><th>▶ Only in {}</th><th>= Same</th><th>Total</th><th>Share</th></tr>",
        esc(&names[0]),
        esc(&names[1])
    );
    for g in &hot {
        let name = if g.folder.is_empty() {
            "(top level)".to_owned()
        } else {
            format!(
                "<a href=\"#g-{}\">{}</a>",
                esc(&encode_component(&g.folder)),
                esc(&g.folder)
            )
        };
        let _ = write!(
            out,
            "<tr><td class=\"mono\">{name}</td>{}{}{}{}<td>{}</td><td class=\"mb\">{}</td></tr>",
            cell(g.different),
            cell(g.left_only),
            cell(g.right_only),
            cell(g.same),
            num(mismatched(g) + g.same),
            bar("mbar", g.left_only, g.different, g.right_only, g.same)
        );
    }
    out.push_str("</table>");
    if !clean.is_empty() {
        let shown: Vec<String> = clean
            .iter()
            .take(CLEAN_FOLDERS)
            .map(|g| format!("<span class=\"mono\">{}</span>", esc(dir_label(&g.folder))))
            .collect();
        let _ = write!(
            out,
            "<p class=\"cfg\">Folders that match completely ({}): {}{}</p>",
            num(clean.len()),
            shown.join(", "),
            if clean.len() > CLEAN_FOLDERS {
                " …"
            } else {
                ""
            }
        );
    }
}

fn files_section<'a>(out: &mut String, page: &mut Page<'a>, detail: &[&'a ReportItem]) {
    let names = page.names();
    let of = |state: ItemState| -> Vec<&'a ReportItem> {
        detail
            .iter()
            .copied()
            .filter(|i| i.state == state)
            .collect()
    };
    let (different, left, right, same) = (
        of(ItemState::Different),
        of(ItemState::LeftOnly),
        of(ItemState::RightOnly),
        of(ItemState::Same),
    );
    let _ = write!(
        out,
        "<h2>Files<small>{}</small></h2><div class=\"tools\"><input type=\"search\" id=\"q\" placeholder=\"Search file or folder names…\">",
        files(detail.len())
    );
    for state in [
        ItemState::Different,
        ItemState::LeftOnly,
        ItemState::RightOnly,
        ItemState::Same,
    ] {
        if detail.iter().any(|item| item.state == state) {
            let _ = write!(
                out,
                "<label><input type=\"checkbox\" value=\"{}\" checked>{} {}</label>",
                state_key(state),
                state_symbol(state),
                esc(&state_label(state, names))
            );
        }
    }
    if !different.is_empty() {
        out.push_str("<button type=\"button\" id=\"exp\">Expand all differing lines</button><button type=\"button\" id=\"col\">Collapse all</button>");
    }
    out.push_str("<span class=\"cnt\" id=\"cnt\"></span></div>");

    if !different.is_empty() {
        let _ = write!(
            out,
            "<h3 class=\"sub\"><span class=\"sw d\"></span>≠ Different contents — {}<small>Click a file name to see the differing lines</small></h3>",
            files(different.len())
        );
        let mut by_dir: HashMap<&str, Vec<&ReportItem>> = HashMap::new();
        for &item in &different {
            by_dir.entry(dir_of(&item.rel)).or_default().push(item);
        }
        for dir in sorted_keys(&by_dir) {
            let group = &by_dir[dir];
            let _ = write!(
                out,
                "<div class=\"dgrp box\">{}<h4><span class=\"mono\">📁 {}</span><small>{}</small></h4>",
                page.anchors_for(dir),
                esc(dir_label(dir)),
                files(group.len())
            );
            for item in group {
                out.push_str(&page.file_card(item, page.ids[item.rel.as_str()]));
            }
            out.push_str("</div>");
        }
    }
    page.one_sided(out, &left, ItemState::LeftOnly);
    page.one_sided(out, &right, ItemState::RightOnly);
    if !same.is_empty() {
        let _ = write!(
            out,
            "<h3 class=\"sub\"><span class=\"sw s\"></span>= Same — {}</h3>",
            files(same.len())
        );
        let mut by_dir: HashMap<&str, Vec<&ReportItem>> = HashMap::new();
        for &item in &same {
            by_dir.entry(dir_of(&item.rel)).or_default().push(item);
        }
        for dir in sorted_keys(&by_dir) {
            let group = &by_dir[dir];
            let _ = write!(
                out,
                "<div class=\"og box\"><div class=\"oh\"><span class=\"mono\">📁 {}</span><span class=\"info\">{}</span></div><ul class=\"fl\">",
                esc(dir_label(dir)),
                files(group.len())
            );
            for item in group {
                let _ = write!(
                    out,
                    "<li class=\"it\" id=\"f{}\" data-st=\"same\" data-p=\"{}\">{} <span class=\"sz\">{}</span></li>",
                    page.ids[item.rel.as_str()],
                    esc(&item.rel.to_lowercase()),
                    esc(base_of(&item.rel)),
                    human_bytes(item.left.as_ref().map_or(0, |s| s.len))
                );
            }
            out.push_str("</ul></div>");
        }
    }
}

fn legend(out: &mut String, options: &ReportOptions) {
    let names = &options.names;
    let _ = write!(
        out,
        "<h2>Legend</h2><dl class=\"gl\">\
<dt>A / B</dt><dd>A = {}, B = {} (the names given when the report was made)</dd>\
<dt><span class=\"sw d\"></span> ≠ Different</dt><dd>The file is on both sides, but its contents do not match by the criterion used</dd>\
<dt><span class=\"sw a\"></span> ◀ Only in A · <span class=\"sw b\"></span> ▶ Only in B</dt><dd>The file is on one side only; the other side does not have it (it may have been deleted, moved or added)</dd>\
<dt><span class=\"sw s\"></span> = Same</dt><dd>The contents match, including files that differ only in EOL (CRLF or LF), a BOM or parts set to be unimportant</dd>\
<dt>Changed / Only in A / Only in B (lines)</dt><dd>Changed = lines at the same place with different text; Only in A = lines B does not have; Only in B = lines A does not have · a place is a group of differing lines next to each other</dd>\
<dt>Colours in the line table</dt><dd>Yellow = changed (the differing characters are darker), red = only in A, green = only in B, purple = differs only in unimportant parts, stripes = this side has no such line · ⍽ / LRM = invisible characters</dd>",
        esc(&names[0]),
        esc(&names[1])
    );
    if options.mask {
        out.push_str(
            "<dt>•••••• (hidden #xxxx)</dt><dd>A value that looks like a password or key is hidden; the four characters are its fingerprint: different fingerprints mean different values</dd>\
<dt>Private keys</dt><dd>Each line of a -----BEGIN … PRIVATE KEY----- block is hidden with its own fingerprint; the BEGIN and END lines stay</dd>\
<dt>Multi-line values</dt><dd>Each line of a YAML block value under a password or key name (password: |) is hidden the same way</dd>\
<dt>Not hidden</dt><dd>Other secrets that span lines are not detected</dd>",
        );
    }
    out.push_str(
        "<dt>Modified later</dt><dd>The file on that side was modified later than the other (the time does not say which side is right)</dd></dl>",
    );
}
