use super::*;

fn yaml(text: &str) -> Result<Sorted, SortError> {
    sort_keys(text, SortKind::Yaml, &CancelToken::new())
}

fn out(text: &str) -> String {
    yaml(text).unwrap().text
}

#[test]
fn keys_are_sorted_at_every_level_and_sequences_keep_their_order() {
    assert_eq!(
        out("b: 1\na:\n  z: 1\n  y: [3, 1, 2]\n  x:\n    - q: 1\n      p: 2\n    - 7\n"),
        "a:\n  x:\n    - p: 2\n      q: 1\n    - 7\n  y:\n    - 3\n    - 1\n    - 2\n  z: 1\nb: 1\n"
    );
    assert_eq!(
        out("- [a, b]\n- {y: 1, x: 2}\n- {}\n- []\n"),
        "- - a\n  - b\n- x: 2\n  y: 1\n- {}\n- []\n"
    );
}

#[test]
fn plain_and_quoted_scalars_stay_different() {
    let pairs = [
        ("a: no", "a: \"no\""),
        ("a: yes", "a: 'yes'"),
        ("a: 1", "a: \"1\""),
        ("a: ~", "a: null"),
        ("a: null", "a:"),
        ("a: ~", "a:"),
        ("a:", "a: \"\""),
        ("a: Null", "a: null"),
        ("a: on", "a: true"),
    ];
    for (left, right) in pairs {
        assert_ne!(out(left), out(right), "{left} against {right}");
    }
    assert_eq!(out("a: 'x'"), out("a: \"x\""));
}

#[test]
fn numbers_and_versions_are_copied_as_written() {
    let values = [
        "1.0", "1", "0x1F", "31", "1e3", "1000", "012", "12", "1_000", "1000",
    ];
    let outs: Vec<String> = values.iter().map(|v| out(&format!("a: {v}"))).collect();
    for i in 0..values.len() {
        for j in 0..i {
            if values[i] != values[j] {
                assert_ne!(outs[i], outs[j], "{} against {}", values[i], values[j]);
            }
        }
    }
}

#[test]
fn anchors_are_expanded_and_noted() {
    let sorted = yaml("a: &x {b: 1}\nc: *x\n").unwrap();
    assert_eq!(sorted.text, "a:\n  b: 1\nc:\n  b: 1\n");
    assert!(sorted.notes.contains(&SortNote::AnchorsExpanded));
    let merged = out("base: &x {b: 1}\nuse:\n  <<: *x\n  c: 2\n");
    assert_eq!(merged, "base:\n  b: 1\nuse:\n  <<:\n    b: 1\n  c: 2\n");
}

#[test]
fn an_alias_to_an_unfinished_anchor_is_a_syntax_error() {
    match yaml("a: &x\n  b: *x\n") {
        Err(SortError::Syntax { line, .. }) => assert_eq!(line, 2),
        other => panic!("{other:?}"),
    }
}

#[test]
fn an_alias_bomb_is_refused() {
    let mut text = String::from("a0: &a0 [x, x, x, x, x, x, x, x, x, x]\n");
    for level in 1..=9 {
        let refs = format!("*a{} ", level - 1).repeat(10);
        text.push_str(&format!(
            "a{level}: &a{level} [{}]\n",
            refs.trim().replace(' ', ", ")
        ));
    }
    let started = std::time::Instant::now();
    assert!(matches!(yaml(&text), Err(SortError::TooBig)));
    assert!(started.elapsed().as_secs() < 5, "refused quickly");
}

#[test]
fn multiple_documents_are_each_sorted_and_noted() {
    let sorted = yaml("b: 1\na: 1\n---\nz: 1\ny: 2\n").unwrap();
    assert_eq!(sorted.text, "a: 1\nb: 1\n---\ny: 2\nz: 1\n");
    assert_eq!(sorted.notes, vec![SortNote::Documents(2)]);
    let empty = yaml("a: 1\n---\n").unwrap();
    assert_eq!(empty.text, "a: 1\n---\n");
    assert_eq!(empty.notes, vec![SortNote::Documents(2)]);
}

#[test]
fn comments_are_dropped_and_noted() {
    let sorted = yaml("# top\nb: 1 # one\na: 2\n").unwrap();
    assert_eq!(sorted.text, "a: 2\nb: 1\n");
    assert!(sorted.notes.contains(&SortNote::CommentsDropped));
    assert!(yaml("a: 1\n").unwrap().notes.is_empty());
}

#[test]
fn a_long_run_of_comments_still_parses() {
    let text = format!("{}b: 1\na: 2\n", "# note\n".repeat(200));
    let sorted = yaml(&text).unwrap();
    assert_eq!(sorted.text, "a: 2\nb: 1\n");
}

#[test]
fn duplicate_keys_are_kept_and_reported() {
    let sorted = yaml("a: 1\nb: 2\na: 3\n").unwrap();
    assert_eq!(sorted.text, "a: 1\na: 3\nb: 2\n");
    assert_eq!(
        sorted.notes,
        vec![SortNote::DuplicateKey {
            key: "a".into(),
            line: 3
        }]
    );
}

#[test]
fn a_syntax_error_names_the_line() {
    for (text, line) in [
        ("a: 1\nb:\n\t- x\n", 3),
        ("a: [1, 2\nb: 3\n", 2),
        ("a:\n  b: 1\n c: 2\n", 3),
    ] {
        match yaml(text) {
            Err(SortError::Syntax {
                line: got, message, ..
            }) => {
                assert_eq!(got, line, "{text:?}");
                assert!(!message.is_empty());
            }
            other => panic!("{text:?}: {other:?}"),
        }
    }
}

#[test]
fn a_complex_key_is_refused() {
    assert!(matches!(
        yaml("? [a, b]\n: v\n"),
        Err(SortError::Unsupported(_))
    ));
}

#[test]
fn block_scalars_and_quoted_twins_compare_equal() {
    assert_eq!(out("a: |\n  a\n  b\n"), out("a: \"a\\nb\\n\"\n"));
    assert_eq!(out("a: >\n  a\n  b\n"), out("a: \"a b\\n\"\n"));
    assert_eq!(out("a: |\n  a\n  b\n"), "a: \"a\\nb\\n\"\n");
}

#[test]
fn nesting_past_256_levels_is_refused() {
    let deep = |n: usize| format!("{}{}", "[".repeat(n), "]".repeat(n));
    assert!(yaml(&deep(256)).is_ok());
    assert_eq!(yaml(&deep(300)), Err(SortError::TooDeep));
    assert_eq!(yaml(&"- ".repeat(300)), Err(SortError::TooDeep));
}

#[test]
fn json_in_a_yaml_file_is_sorted_as_yaml() {
    assert_eq!(out("{\"b\": 1, \"a\": 2}"), "a: 2\nb: 1");
}

#[test]
fn tags_are_written_so_a_tag_still_shows_as_a_difference() {
    assert_ne!(out("a: !!str 1"), out("a: 1"));
    assert_eq!(out("a: !!str 1\n"), "a: !!str 1\n");
}

#[test]
fn an_empty_key_and_the_trailing_newline() {
    assert_eq!(out("b: 1\na:\n"), "a:\nb: 1\n");
    assert_eq!(out("b: 1\na: 2"), "a: 2\nb: 1");
}

#[test]
fn a_quoted_key_is_plain_only_when_that_cannot_change_its_meaning() {
    assert_eq!(out("\"a\": 1"), "a: 1\n".trim_end());
    assert_ne!(out("\"no\": 1"), out("no: 1"));
    assert_ne!(out("\"1\": 1"), out("1: 1"));
    assert_eq!(out("\"a b\": 1"), "\"a b\": 1");
}

#[test]
fn a_final_cr_counts_as_a_final_break() {
    assert_eq!(out("b: 1\na: 2\r"), "a: 2\nb: 1\n");
}

#[test]
fn a_small_file_cannot_print_past_the_output_limit() {
    let text = format!(
        "{}{}x{}",
        "[".repeat(150),
        "x, ".repeat(300_000),
        "]".repeat(150)
    );
    assert!(text.len() < 1_100_000);
    assert!(matches!(yaml(&text), Err(SortError::TooBig)));
}

#[test]
fn a_flood_of_comments_is_refused_not_buffered_without_end() {
    let text = format!("c:\n{}  x\nb: 1\n", "# note\n".repeat(20_000));
    assert!(matches!(yaml(&text), Err(SortError::TooBig)));
}

#[test]
fn keys_of_the_same_text_order_by_how_they_are_written() {
    let left = yaml("\"1\": a\n1: b\n").unwrap();
    let right = yaml("1: b\n\"1\": a\n").unwrap();
    assert_eq!(left.text, right.text);
    assert!(
        left.notes.is_empty() && right.notes.is_empty(),
        "{:?} {:?}",
        left.notes,
        right.notes
    );
    // The very same key twice is still a duplicate.
    let twice = yaml("1: a\n1: b\n\"1\": c\n").unwrap();
    assert_eq!(twice.notes.len(), 1);
}

#[test]
fn a_dash_key_and_an_empty_key_in_a_sequence_print_differently() {
    let dash = out("-: v\n");
    let empty = out("- : v\n");
    assert_ne!(dash, empty);
    assert_eq!(dash, "-: v\n");
}

#[test]
fn a_percent_encoded_tag_is_written_encoded_again() {
    assert_eq!(out("a: !x%20y 1\n"), "a: !x%20y 1\n");
}

#[test]
fn a_version_directive_is_noted() {
    let sorted = yaml("%YAML 1.1\n---\na: 1\n").unwrap();
    assert!(
        sorted.notes.contains(&SortNote::YamlVersion(1, 1)),
        "{:?}",
        sorted.notes
    );
    assert!(yaml("a: 1\n").unwrap().notes.is_empty());
}

#[test]
fn an_alias_cannot_push_the_nesting_past_the_limit() {
    let deep = format!("a: &x {}{}\n", "[".repeat(200), "]".repeat(200));
    let under = format!("b: {}*x{}\n", "[".repeat(100), "]".repeat(100));
    assert_eq!(yaml(&format!("{deep}{under}")), Err(SortError::TooDeep));
    assert!(yaml(&format!("{deep}b: [*x]\n")).is_ok());
}

#[test]
fn chomping_indicators_keep_their_newlines() {
    assert_eq!(out("a: |-\n  x\n"), "a: \"x\"\n");
    assert_eq!(out("a: |+\n  x\n\n"), "a: \"x\\n\\n\"\n");
    assert_eq!(out("a: |\n  x\n"), "a: \"x\\n\"\n");
    assert_ne!(out("a: |-\n  x\n"), out("a: |\n  x\n"));
}
