use super::*;
use duet_core::folder::classify::{Note, Warning};
use duet_core::folder::entry::{Entry, EntryKind};
use duet_core::folder::tree::build_tree;
use std::time::UNIX_EPOCH;

#[test]
fn chips_count_what_their_filter_shows() {
    let counts = Counts {
        all: 7,
        different: 1,
        similar: 1,
        left_only: 1,
        right_only: 2,
        same: 2,
        ..Counts::default()
    };
    let count = |f: FolderFilter| f.count(&counts);
    assert_eq!(
        [
            FolderFilter::All,
            FolderFilter::Mismatch,
            FolderFilter::Different,
            FolderFilter::LeftOnly,
            FolderFilter::RightOnly,
            FolderFilter::Same
        ]
        .map(count),
        [7, 4, 1, 1, 2, 3]
    );
    assert_eq!(FolderFilter::Mismatch.chip_label(), "Mismatched");
    assert_eq!(FolderFilter::Different.chip_label(), "Changed");
    assert_eq!(FolderFilter::LeftOnly.chip_id(), "folder-chip-left-only");
    assert_eq!(
        FolderFilter::Mismatch.label(),
        "Mismatches",
        "the status line keeps its word"
    );
}

fn entry(rel: &str, kind: EntryKind, len: u64) -> Entry {
    Entry {
        rel: rel.into(),
        kind,
        len,
        modified: Some(UNIX_EPOCH),
        error: None,
        link_target: None,
    }
}

/// a/ (same.txt same, x.txt different), b/ left-only with only.txt, new.txt right-only,
/// top.txt same.
#[test]
fn folder_navigation_wraps_and_reports_it() {
    let mut model = sample();
    let first = model.next_difference_wrapping().unwrap();
    assert!(!first.1);
    let mut last = first;
    while let Some(step) = model.next_difference_wrapping() {
        if step.1 {
            assert_eq!(
                step.0, first.0,
                "wrapping lands on the first difference again"
            );
            break;
        }
        last = step;
    }
    assert_ne!(last.0, first.0);
    assert_eq!(
        model.prev_difference_wrapping().map(|s| s.1),
        Some(true),
        "back over the top wraps to the end"
    );
}

#[test]
fn folders_count_their_differing_descendants() {
    let model = sample();
    let index = |rel: &str| {
        model
            .comparison()
            .nodes
            .iter()
            .position(|n| n.rel == rel)
            .unwrap() as u32
    };
    assert_eq!(
        model.differing_below(index("a")),
        1,
        "a/x.txt differs, a/same.txt does not"
    );
    assert_eq!(
        model.differing_below(index("b")),
        1,
        "the left-only b/only.txt"
    );
    assert_eq!(model.differing_below(index("top.txt")), 0);
}

fn visible_rels(model: &FolderModel) -> Vec<&str> {
    (0..model.visible_len())
        .map(|r| model.node_at(r).rel.as_str())
        .collect()
}

#[test]
fn the_name_filter_keeps_matches_and_the_folders_above_them() {
    let mut model = sample();
    model.set_filter(FolderFilter::All);
    let everything = model.visible_len();
    model.set_name_filter("X.T");
    assert_eq!(visible_rels(&model), ["a", "a/x.txt"]);
    model.set_name_filter("only");
    assert_eq!(
        visible_rels(&model),
        ["b", "b/only.txt"],
        "a closed folder opens to show a match"
    );
    model.set_name_filter("  ");
    assert_eq!(model.visible_len(), everything);
}

#[test]
fn navigation_skips_differences_the_name_filter_hides() {
    let mut model = sample();
    model.set_name_filter("only");
    let first = model
        .next_difference_wrapping()
        .map(|(row, _)| model.node_at(row).rel.clone());
    assert_eq!(
        first.as_deref(),
        Some("b"),
        "the left-only folder is the stop"
    );
    let again = model.next_difference_wrapping();
    assert_eq!(
        again.map(|(row, wrapped)| (model.node_at(row).rel.clone(), wrapped)),
        Some(("b".to_owned(), true)),
        "with one visible stop, F8 wraps back to it instead of sticking"
    );
    assert!(model.prev_difference_wrapping().is_some());
}

#[test]
fn home_and_end_select_the_first_and_last_rows() {
    let mut model = sample();
    model.select_last();
    assert_eq!(model.selected_row(), Some(model.visible_len() - 1));
    model.select_first();
    assert_eq!(model.selected_row(), Some(0));
}

#[test]
fn one_sided_filters_mirror() {
    assert_eq!(FolderFilter::LeftOnly.mirrored(), FolderFilter::RightOnly);
    assert_eq!(FolderFilter::RightOnly.mirrored(), FolderFilter::LeftOnly);
    assert_eq!(FolderFilter::Mismatch.mirrored(), FolderFilter::Mismatch);
}

#[test]
fn counts_total_the_files_and_bytes_on_each_side() {
    let counts = sample().counts();
    // left: a/same.txt, a/x.txt, b/only.txt, top.txt; right: a/same.txt, a/x.txt (2 bytes),
    // new.txt, top.txt
    assert_eq!((counts.left_files, counts.right_files), (4, 4));
    assert_eq!((counts.left_bytes, counts.right_bytes), (4, 5));
}

fn sample() -> FolderModel {
    let file = |rel, len| entry(rel, EntryKind::File, len);
    let dir = |rel| entry(rel, EntryKind::Dir, 0);
    let left = vec![
        dir("a"),
        file("a/same.txt", 1),
        file("a/x.txt", 1),
        dir("b"),
        file("b/only.txt", 1),
        file("top.txt", 1),
    ];
    let right = vec![
        dir("a"),
        file("a/same.txt", 1),
        file("a/x.txt", 2),
        file("new.txt", 1),
        file("top.txt", 1),
    ];
    FolderModel::new(build_tree(left, right, true))
}

fn rels(model: &FolderModel) -> Vec<&str> {
    (0..model.visible_len())
        .map(|row| model.node_at(row).rel.as_str())
        .collect()
}

#[test]
fn mismatch_filter_is_the_default_and_hides_same_files() {
    let model = sample();
    assert_eq!(model.filter(), FolderFilter::Mismatch);
    assert_eq!(rels(&model), ["a", "a/x.txt", "b", "new.txt"]);
}

#[test]
fn each_filter_keeps_folders_that_lead_to_matches() {
    let mut model = sample();
    model.set_filter(FolderFilter::All);
    assert_eq!(
        rels(&model),
        ["a", "a/same.txt", "a/x.txt", "b", "new.txt", "top.txt"]
    );
    model.set_filter(FolderFilter::Same);
    assert_eq!(rels(&model), ["a", "a/same.txt", "top.txt"]);
    model.set_filter(FolderFilter::LeftOnly);
    assert_eq!(rels(&model), ["b"]);
    model.set_filter(FolderFilter::RightOnly);
    assert_eq!(rels(&model), ["new.txt"]);
    model.set_filter(FolderFilter::Different);
    assert_eq!(rels(&model), ["a", "a/x.txt"]);
}

#[test]
fn counts_cover_leaves_only() {
    let c = sample().counts();
    assert_eq!(
        (
            c.all,
            c.same,
            c.different,
            c.left_only,
            c.right_only,
            c.errors,
            c.pending
        ),
        (5, 2, 1, 1, 1, 0, 0)
    );
}

#[test]
fn difference_navigation_stops_once_per_orphan_folder() {
    let mut model = sample();
    assert_eq!(model.next_difference(), Some(1));
    assert_eq!(model.selected_node().unwrap().rel, "a/x.txt");
    assert_eq!(
        model.next_difference(),
        Some(2),
        "orphan folder b, not each file in it"
    );
    assert_eq!(model.next_difference(), Some(3));
    assert_eq!(model.selected_node().unwrap().rel, "new.txt");
    assert_eq!(model.next_difference(), None, "no wrap-around");
    assert_eq!(model.prev_difference(), Some(2));
    assert_eq!(model.prev_difference(), Some(1));
    assert_eq!(model.prev_difference(), None);
}

#[test]
fn navigation_reveals_differences_inside_collapsed_folders() {
    let mut model = sample();
    model.collapse_all();
    assert_eq!(rels(&model), ["a", "b", "new.txt"]);
    assert_eq!(model.next_difference(), Some(1));
    assert_eq!(model.selected_node().unwrap().rel, "a/x.txt");
    assert!(model.is_expanded(0));
}

#[test]
fn expand_collapse_and_selection_moves() {
    let mut model = sample();
    model.select_row(1);
    model.collapse_selected();
    assert_eq!(
        model.selected_node().unwrap().rel,
        "a",
        "collapse on a file selects its folder"
    );
    model.collapse_selected();
    assert_eq!(rels(&model), ["a", "b", "new.txt"]);
    model.move_selection(1);
    model.expand_selected();
    assert_eq!(rels(&model), ["a", "b", "b/only.txt", "new.txt"]);
    model.move_selection(10);
    assert_eq!(model.selected_row(), Some(3), "moves clamp at the end");
    model.move_selection(-10);
    assert_eq!(model.selected_row(), Some(0));
}

#[test]
fn carry_over_keeps_expansion_filter_and_selection_by_path() {
    let mut old = sample();
    old.set_filter(FolderFilter::All);
    old.select_row(3);
    old.expand_selected();
    old.select_row(4);
    assert_eq!(old.selected_node().unwrap().rel, "b/only.txt");
    let mut fresh = sample();
    fresh.carry_over(&old);
    assert_eq!(fresh.filter(), FolderFilter::All);
    assert_eq!(fresh.selected_node().unwrap().rel, "b/only.txt");
    assert!(rels(&fresh).contains(&"b/only.txt"));
}

#[test]
fn a_swept_range_is_the_base_plus_the_rows_between_and_shrinks_back() {
    let mut m = sample();
    m.set_filter(FolderFilter::All);
    m.expand_all();
    m.select_row(0);
    m.toggle_chosen(5);
    let base = m.chosen_snapshot();
    m.set_range_over(&base, 2, 4);
    let chosen: Vec<_> = (0..m.visible_len()).filter(|&r| m.is_chosen(r)).collect();
    assert_eq!(chosen, [0, 2, 3, 4, 5], "base rows 0 and 5 plus 2..=4");
    assert_eq!(m.selected_row(), Some(4));
    m.set_range_over(&base, 2, 3);
    assert!(
        !m.is_chosen(4) && m.is_chosen(3) && m.is_chosen(5),
        "shrinks"
    );
    m.set_range_over(&base, 2, 0);
    let chosen: Vec<_> = (0..m.visible_len()).filter(|&r| m.is_chosen(r)).collect();
    assert_eq!(chosen, [0, 1, 2, 5], "sweeping upwards past the anchor");
}

#[test]
fn ctrl_toggles_shift_extends_and_a_plain_click_resets() {
    let mut m = sample();
    m.set_filter(FolderFilter::All);
    m.expand_all();
    m.select_row(1);
    m.toggle_chosen(3);
    assert_eq!(m.chosen_count(), 2);
    assert!(m.is_chosen(1) && m.is_chosen(3) && !m.is_chosen(2));
    m.extend_to(5, false);
    assert_eq!(
        (3..=5).filter(|&r| m.is_chosen(r)).count(),
        3,
        "anchor 3 to 5"
    );
    assert!(!m.is_chosen(1), "shift without ctrl replaces the set");
    m.toggle_chosen(1);
    m.extend_to(0, true);
    assert!(
        m.is_chosen(0) && m.is_chosen(1) && m.is_chosen(4),
        "ctrl+shift adds"
    );
    m.select_row(2);
    assert_eq!(m.chosen_count(), 1);
    assert_eq!(m.selected_row(), Some(2));
}

#[test]
fn shift_arrows_grow_and_shrink_from_the_anchor() {
    let mut m = sample();
    m.set_filter(FolderFilter::All);
    m.expand_all();
    m.select_row(2);
    m.extend_by(2);
    assert_eq!(m.chosen_count(), 3);
    assert_eq!(m.selected_row(), Some(4));
    m.extend_by(-3);
    assert_eq!(m.selected_row(), Some(1));
    assert_eq!(m.chosen_count(), 2, "rows 1 and 2");
}

#[test]
fn select_all_takes_the_visible_rows_only() {
    let mut m = sample();
    m.collapse_all();
    m.select_all();
    assert_eq!(m.chosen_count(), m.visible_len());
}

#[test]
fn picked_drops_rows_inside_a_chosen_folder() {
    let mut m = sample();
    m.set_filter(FolderFilter::All);
    m.expand_all();
    m.select_all();
    let picked: Vec<&str> = m.picked().iter().map(|&n| m.node(n).rel.as_str()).collect();
    // Only top-level rows remain: nothing below a picked folder.
    assert!(picked.iter().all(|rel| !rel.contains('/')), "{picked:?}");
    assert_eq!(picked, ["a", "b", "new.txt", "top.txt"]);
}

#[test]
fn an_empty_set_picks_the_cursor() {
    let mut m = sample();
    m.set_filter(FolderFilter::All);
    m.expand_all();
    m.select_row(2);
    m.toggle_chosen(2);
    assert_eq!(m.chosen_count(), 0);
    assert_eq!(m.selected_row(), Some(2));
    assert_eq!(m.picked(), vec![m.node_index(2)]);
}

#[test]
fn hiding_rows_prunes_them_from_the_set_and_carry_over_keeps_it_by_path() {
    let mut m = sample();
    m.set_filter(FolderFilter::All);
    m.expand_all();
    m.select_all();
    let before = m.chosen_count();
    m.collapse_all();
    assert!(m.chosen_count() < before, "collapsed rows leave the set");
    assert!(m.chosen_count() >= 1);
    let mut fresh = sample();
    fresh.carry_over(&m);
    assert_eq!(fresh.chosen_rels(), m.chosen_rels());
}

#[test]
fn passes_filters_ignores_collapse_but_not_chips_or_names() {
    let mut m = sample();
    m.collapse_all();
    let same = (0..m.comparison().nodes.len() as u32)
        .find(|&n| m.node(n).status == Status::Same && !m.node(n).is_dir)
        .unwrap();
    m.set_filter(FolderFilter::All);
    assert!(m.passes_filters(same));
    m.set_filter(FolderFilter::Mismatch);
    assert!(!m.passes_filters(same));
    m.set_filter(FolderFilter::All);
    m.set_name_filter("zzz");
    assert!(!m.passes_filters(same), "the name filter hides it too");
}

#[test]
fn a_hundred_thousand_nodes_build_quickly() {
    let mut left = Vec::new();
    for d in 0..1_000 {
        left.push(entry(&format!("d{d:04}"), EntryKind::Dir, 0));
        for f in 0..99 {
            left.push(entry(&format!("d{d:04}/f{f:02}.txt"), EntryKind::File, 1));
        }
    }
    let mut right = left.clone();
    right[5].len = 2;
    let start = std::time::Instant::now();
    let mut model = FolderModel::new(build_tree(left, right, true));
    model.set_filter(FolderFilter::All);
    assert_eq!(
        model.visible_len(),
        1_000 + 99,
        "only the folder with a difference is open"
    );
    assert!(start.elapsed() < std::time::Duration::from_secs(2));
}

#[test]
fn filters_have_keys() {
    for filter in FolderFilter::ALL {
        assert_eq!(FolderFilter::from_key(filter.key()), Some(filter));
    }
    assert_eq!(FolderFilter::from_key("x"), None);
    let keys: Vec<_> = FolderFilter::ALL.iter().map(|f| f.key()).collect();
    assert_eq!(
        keys,
        ["all", "mismatch", "different", "left", "right", "same"]
    );
}

/// A tree of file pairs named in `rels` (left, right), with the statuses and notes set by hand.
fn hand_built(
    rels: &[(&str, &str, Status, Option<Note>)],
    right_time: impl Fn(&str) -> std::time::SystemTime,
) -> FolderModel {
    let file = |rel: &str, time| Entry {
        modified: Some(time),
        ..entry(rel, EntryKind::File, 1)
    };
    let left = rels
        .iter()
        .map(|(l, _, _, _)| file(l, UNIX_EPOCH))
        .collect();
    let right = rels
        .iter()
        .map(|(_, r, _, _)| file(r, right_time(r)))
        .collect();
    let mut cmp = build_tree(left, right, true);
    for node in &mut cmp.nodes {
        let (_, _, status, note) = rels
            .iter()
            .find(|(l, ..)| l.eq_ignore_ascii_case(&node.rel))
            .unwrap();
        node.status = status.clone();
        node.note = note.clone().map(Box::new);
    }
    FolderModel::new(cmp)
}

#[test]
fn similar_files_are_neither_stops_nor_mismatches() {
    let mut model = hand_built(
        &[
            ("a.txt", "a.txt", Status::Similar { newer: None }, None),
            ("b.txt", "b.txt", Status::Different { newer: None }, None),
        ],
        |_| UNIX_EPOCH,
    );
    assert_eq!(rels(&model), ["b.txt"], "Mismatched leaves the = file out");
    let at = |model: &FolderModel, step: Option<(usize, bool)>| {
        step.map(|(row, wrapped)| (model.node_at(row).rel.clone(), wrapped))
    };
    let step = model.next_difference_wrapping();
    assert_eq!(at(&model, step), Some(("b.txt".to_owned(), false)));
    let step = model.next_difference_wrapping();
    assert_eq!(
        at(&model, step),
        Some(("b.txt".to_owned(), true)),
        "wraps back to the same stop"
    );
    let index = model
        .comparison()
        .nodes
        .iter()
        .position(|n| n.rel == "a.txt")
        .unwrap() as u32;
    assert_eq!(model.differing_below(index), 0);
    model.set_filter(FolderFilter::Same);
    assert_eq!(rels(&model), ["a.txt"], "the Same chip lists it");
    model.set_filter(FolderFilter::Different);
    assert_eq!(rels(&model), ["b.txt"]);
}

#[test]
fn counts_cover_each_kind_of_verdict() {
    let note = |soft, warning| Note {
        soft,
        warning,
        ..Note::default()
    };
    let hour = std::time::Duration::from_secs(7_200);
    let model = hand_built(
        &[
            (
                "soft.txt",
                "soft.txt",
                Status::Similar { newer: None },
                Some(note(true, None)),
            ),
            (
                "fmt.txt",
                "fmt.txt",
                Status::Similar { newer: None },
                Some(note(false, None)),
            ),
            ("time.txt", "time.txt", Status::Same, None),
            (
                "run.sh",
                "run.sh",
                Status::Different { newer: None },
                Some(note(false, Some(Warning::Script))),
            ),
            (
                "odd.txt",
                "odd.txt",
                Status::Different { newer: None },
                Some(note(false, Some(Warning::OddSpace))),
            ),
            (
                "Case.txt",
                "case.txt",
                Status::Different { newer: None },
                Some(note(false, Some(Warning::CaseOnly))),
            ),
            (
                "Diff.txt",
                "diff.txt",
                Status::Different { newer: None },
                None,
            ),
        ],
        |rel| {
            if rel == "time.txt" {
                UNIX_EPOCH + hour
            } else {
                UNIX_EPOCH
            }
        },
    );
    let c = model.counts();
    assert_eq!(
        (
            c.soft,
            c.format_only,
            c.time_only,
            c.script,
            c.odd_space,
            c.case_names
        ),
        (1, 1, 1, 1, 1, 2)
    );
    assert_eq!((c.similar, c.same, c.different), (2, 1, 4));
}

fn index_of(model: &FolderModel, rel: &str) -> u32 {
    model
        .comparison()
        .nodes
        .iter()
        .position(|n| n.rel == rel)
        .unwrap() as u32
}

#[test]
fn folders_count_shown_files_and_break_them_down() {
    let mut model = sample();
    let (a, b) = (index_of(&model, "a"), index_of(&model, "b"));
    assert_eq!(model.filter(), FolderFilter::Mismatch);
    assert_eq!(model.files_shown_below(a), 1, "only a/x.txt differs");
    assert_eq!(model.files_shown_below(b), 1);
    model.set_filter(FolderFilter::All);
    assert_eq!(model.files_shown_below(a), 2);
    assert_eq!(
        model.breakdown(a),
        Breakdown {
            different: 1,
            left_only: 0,
            right_only: 0,
            same: 1
        }
    );
    assert_eq!(
        model.breakdown(b),
        Breakdown {
            left_only: 1,
            ..Breakdown::default()
        }
    );
    model.set_name_filter("same");
    assert_eq!(model.files_shown_below(a), 1, "the path filter narrows it");
    assert_eq!(
        model.breakdown(a).same + model.breakdown(a).different,
        2,
        "the breakdown ignores the filters"
    );
}

#[test]
fn empty_folders_are_not_counted_as_files() {
    let file = |rel, len| entry(rel, EntryKind::File, len);
    let dir = |rel| entry(rel, EntryKind::Dir, 0);
    let left = vec![dir("d"), file("d/f", 1), dir("e")];
    let mut model = FolderModel::new(build_tree(left, Vec::new(), true));
    model.set_filter(FolderFilter::All);
    let root_dirs = (index_of(&model, "d"), index_of(&model, "e"));
    assert_eq!(model.files_shown_below(root_dirs.0), 1);
    assert_eq!(model.files_shown_below(root_dirs.1), 0);
    assert_eq!(model.breakdown(root_dirs.1), Breakdown::default());
}

fn timed(rel: &str, kind: EntryKind, len: u64, secs: u64) -> Entry {
    Entry {
        modified: Some(UNIX_EPOCH + std::time::Duration::from_secs(secs)),
        ..entry(rel, kind, len)
    }
}

#[test]
fn folders_total_their_files_and_take_the_newest_time() {
    let left = vec![
        timed("d", EntryKind::Dir, 0, 50),
        timed("d/a", EntryKind::File, 3, 10),
        timed("d/b", EntryKind::File, 4, 20),
        timed("e", EntryKind::Dir, 0, 5),
    ];
    let right = vec![timed("e", EntryKind::Dir, 0, 6)];
    let model = FolderModel::new(build_tree(left, right, true));
    let at = |secs| UNIX_EPOCH + std::time::Duration::from_secs(secs);
    let (d, e) = (index_of(&model, "d"), index_of(&model, "e"));
    assert_eq!(model.folder_size(d, true), Some(7));
    assert_eq!(model.folder_newest(d, true), Some(at(20)));
    assert_eq!(model.folder_size(e, true), Some(0));
    assert_eq!(model.folder_newest(e, true), Some(at(5)), "its own time");
    assert_eq!(model.folder_newest(e, false), Some(at(6)));
    assert_eq!(model.folder_size(d, false), None, "d is left only");
    assert_eq!(model.folder_newest(d, false), None);
}

#[test]
fn the_path_filter_matches_folders_and_their_files() {
    let mut model = sample();
    model.set_filter(FolderFilter::All);
    model.set_name_filter("a/");
    assert_eq!(visible_rels(&model), ["a", "a/same.txt", "a/x.txt"]);
}

fn flat(model: &mut FolderModel) {
    model.set_filter(FolderFilter::All);
    model.set_layout(Layout::Flat);
}

#[test]
fn the_flat_list_holds_files_sorted_by_the_chosen_column() {
    let mut model = sample();
    flat(&mut model);
    assert_eq!(model.layout(), Layout::Flat);
    assert_eq!(model.sort(), (SortKey::Path, false));
    assert_eq!(
        rels(&model),
        ["a/same.txt", "a/x.txt", "b/only.txt", "new.txt", "top.txt"]
    );
    model.sort_by(SortKey::Path);
    assert_eq!(model.sort(), (SortKey::Path, true));
    assert_eq!(
        rels(&model),
        ["top.txt", "new.txt", "b/only.txt", "a/x.txt", "a/same.txt"]
    );
    model.sort_by(SortKey::RightSize);
    assert_eq!(model.sort(), (SortKey::RightSize, false));
    assert_eq!(
        rels(&model),
        ["b/only.txt", "a/same.txt", "new.txt", "top.txt", "a/x.txt"],
        "missing first, then 1, 1, 1 by path, then 2"
    );
    model.sort_by(SortKey::RightSize);
    assert_eq!(
        rels(&model),
        ["a/x.txt", "a/same.txt", "new.txt", "top.txt", "b/only.txt"],
        "ties still by path ascending"
    );
    model.sort_by(SortKey::Status);
    assert_eq!(
        rels(&model),
        ["a/x.txt", "b/only.txt", "new.txt", "a/same.txt", "top.txt"]
    );
}

#[test]
fn filters_and_search_apply_to_the_flat_list() {
    let mut model = sample();
    model.set_layout(Layout::Flat);
    assert_eq!(model.filter(), FolderFilter::Mismatch);
    assert_eq!(rels(&model), ["a/x.txt", "b/only.txt", "new.txt"]);
    model.set_name_filter("only");
    assert_eq!(rels(&model), ["b/only.txt"]);
    model.set_name_filter("");
    model.set_filter(FolderFilter::All);
    assert!(
        (0..model.visible_len()).all(|row| !model.node_at(row).is_dir),
        "no folder is ever a flat row"
    );
}

#[test]
fn selection_and_navigation_follow_the_flat_rows() {
    let mut model = sample();
    flat(&mut model);
    model.sort_by(SortKey::RightSize);
    model.sort_by(SortKey::RightSize);
    assert_eq!(model.node_at(0).rel, "a/x.txt");
    model.select_row(0);
    assert_eq!(model.selected_row(), Some(0));
    assert_eq!(model.next_difference_wrapping(), Some((2, false)));
    assert_eq!(model.node_at(2).rel, "new.txt");
    assert_eq!(
        model.next_difference_wrapping(),
        Some((4, false)),
        "a left-only file is a stop even under a left-only folder"
    );
    assert_eq!(model.next_difference_wrapping(), Some((0, true)));
    assert_eq!(model.prev_difference_wrapping(), Some((4, true)));
    assert_eq!(model.prev_difference_wrapping(), Some((2, false)));
    model.select_row(0);
    model.extend_by(1);
    let mut picked: Vec<&str> = model
        .picked()
        .into_iter()
        .map(|n| model.node(n).rel.as_str())
        .collect();
    picked.sort_unstable();
    assert_eq!(picked, ["a/same.txt", "a/x.txt"]);
    assert_eq!(model.chosen_count(), 2);
    assert_eq!(model.selected_row(), Some(1));
}

#[test]
fn the_row_index_survives_sorting_and_filtering() {
    let mut model = sample();
    flat(&mut model);
    model.sort_by(SortKey::LeftTime);
    model.set_filter(FolderFilter::Mismatch);
    model.set_name_filter("x");
    assert!(model.visible_len() > 0);
    for row in 0..model.visible_len() {
        assert_eq!(model.row_of_node(model.node_index(row)), Some(row));
    }
    let top = (0..model.comparison().nodes.len() as u32)
        .find(|&n| model.node(n).rel == "top.txt")
        .unwrap();
    assert_eq!(model.row_of_node(top), None, "a hidden node has no row");
    model.set_layout(Layout::Tree);
    for row in 0..model.visible_len() {
        assert_eq!(model.row_of_node(model.node_index(row)), Some(row));
    }
}

#[test]
fn a_new_model_carries_the_layout_and_the_sort_over() {
    let mut old = sample();
    flat(&mut old);
    old.sort_by(SortKey::RightSize);
    old.select_row(2);
    let mut model = sample();
    model.carry_over(&old);
    assert_eq!(model.layout(), Layout::Flat);
    assert_eq!(model.sort(), (SortKey::RightSize, false));
    assert_eq!(rels(&model), rels(&old));
    assert_eq!(model.selected_row(), Some(2));
}

#[test]
fn a_lost_selection_falls_to_the_first_flat_row() {
    let mut model = sample();
    flat(&mut model);
    model.select_row(3);
    model.set_filter(FolderFilter::LeftOnly);
    assert_eq!(rels(&model), ["b/only.txt"]);
    assert_eq!(model.selected_row(), Some(0));
}

#[test]
fn the_arrow_keys_leave_the_flat_rows_alone() {
    let mut model = sample();
    flat(&mut model);
    model.select_row(1);
    let before = rels(&model).len();
    model.expand_selected();
    model.collapse_selected();
    model.expand_all();
    model.collapse_all();
    assert_eq!(rels(&model).len(), before);
    assert_eq!(model.selected_row(), Some(1));
    model.set_layout(Layout::Tree);
    assert_eq!(
        rels(&model),
        ["a", "a/same.txt", "a/x.txt", "b", "new.txt", "top.txt"],
        "the folders are as they were before the flat list"
    );
}
