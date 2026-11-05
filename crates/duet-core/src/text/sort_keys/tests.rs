use std::path::Path;

use super::*;
use crate::text::format::format_json;

fn json(text: &str) -> Result<Sorted, SortError> {
    sort_keys(text, SortKind::Json, &CancelToken::new())
}

fn json_text(text: &str) -> String {
    json(text).unwrap().text
}

#[test]
fn keys_are_sorted_at_every_level() {
    assert_eq!(
        json_text(r#"{"b":1,"a":{"d":2,"c":3}}"#),
        "{\n  \"a\": {\n    \"c\": 3,\n    \"d\": 2\n  },\n  \"b\": 1\n}"
    );
}

#[test]
fn array_order_is_kept() {
    assert_eq!(
        json_text(r#"{"a":[3,1,2,{"y":1,"x":2}]}"#),
        "{\n  \"a\": [\n    3,\n    1,\n    2,\n    {\n      \"x\": 2,\n      \"y\": 1\n    }\n  ]\n}"
    );
}

#[test]
fn numbers_are_copied_as_written() {
    let numbers = [
        "1.0",
        "1",
        "1e3",
        "1E3",
        "1000",
        "-0",
        "0",
        "12345678901234567890",
        "12345678901234567890.0",
    ];
    let mut seen = std::collections::HashSet::new();
    for number in numbers {
        let out = json_text(&format!(r#"{{"n":{number}}}"#));
        assert!(out.contains(&format!(": {number}\n")), "{number}: {out}");
        assert!(seen.insert(out), "{number} came out like another number");
    }
}

#[test]
fn strings_are_copied_as_written() {
    // An escape is not turned into its character: these are two different texts.
    assert_ne!(
        json_text(r#"{"k":"a"}"#),
        json_text(&format!(r#"{{"k":"{}u0061"}}"#, '\\'))
    );
    assert_eq!(
        json_text(r#"{"b":1,"a\"z":2,"a":3}"#),
        "{\n  \"a\": 3,\n  \"a\\\"z\": 2,\n  \"b\": 1\n}"
    );
    // Byte order: B, then a, then é.
    assert_eq!(
        json_text(r#"{"é":1,"a":2,"B":3}"#),
        "{\n  \"B\": 3,\n  \"a\": 2,\n  \"é\": 1\n}"
    );
    // A key is ordered by its text, not by the quote that ends it: "a" comes before "a!".
    assert_eq!(
        json_text(r#"{"a!":1,"a":2}"#),
        "{\n  \"a\": 2,\n  \"a!\": 1\n}"
    );
}

#[test]
fn duplicate_keys_are_kept_and_reported() {
    let sorted = json("{\"a\":1,\n\"b\":2,\n\"a\":3}").unwrap();
    assert_eq!(
        sorted.text, "{\n  \"a\": 1,\n  \"a\": 3,\n  \"b\": 2\n}",
        "source order among equal keys"
    );
    assert_eq!(
        sorted.notes,
        vec![SortNote::DuplicateKey {
            key: "\"a\"".into(),
            line: 3
        }]
    );
    let three = json(r#"{"a":1,"a":2,"a":3}"#).unwrap();
    assert_eq!(three.notes.len(), 2, "one note per extra entry");
    // Two objects: the notes come in source order.
    let nested = json("{\"x\":{\"q\":1,\"q\":2},\n\"y\":{\"p\":1,\"p\":2}}").unwrap();
    let lines: Vec<usize> = nested
        .notes
        .iter()
        .map(|n| match n {
            SortNote::DuplicateKey { line, .. } => *line,
            _ => 0,
        })
        .collect();
    assert_eq!(lines, [1, 2]);
}

#[test]
fn a_syntax_error_names_the_line() {
    let at = |text: &str| match json(text) {
        Err(SortError::Syntax { line, message, .. }) => (line, message),
        other => panic!("{text:?}: {other:?}"),
    };
    assert_eq!(
        at("{\n  \"a\": 1,\n  \"b\": }"),
        (3, "expected a value".to_owned())
    );
    assert_eq!(
        at("{\"a\": 1,}"),
        (1, "unexpected comma before closing brace".to_owned())
    );
    assert_eq!(at("[1,\n2,]").0, 2);
    assert_eq!(at("{\n// c\n\"a\":1}").0, 2);
    assert_eq!(at("{'a':1}").1, "unexpected character");
    assert_eq!(at("[NaN]").1, "unexpected character");
    assert_eq!(at("1\n2").0, 2);
    assert_eq!(at("1\n2").1, "extra data after the root value");
    assert_eq!(at("[\"open]").1, "unterminated string");
    assert_eq!(at("").1, "expected a value");
    assert_eq!(at("[1,2").1, "unexpected end of the text");
    assert_eq!(at("{\"a\" 1}").1, "expected a colon");
}

#[test]
fn a_bom_is_skipped_and_not_written() {
    let sorted = json("\u{feff}{\"b\":1,\"a\":2}").unwrap();
    assert_eq!(sorted.text, "{\n  \"a\": 2,\n  \"b\": 1\n}");
}

#[test]
fn crlf_input_sorts_like_lf_input() {
    let lf = json("{\n  \"b\": 1,\n  \"a\": [1,\n2]\n}\n").unwrap();
    let crlf = json("{\r\n  \"b\": 1,\r\n  \"a\": [1,\r\n2]\r\n}\r\n").unwrap();
    assert_eq!(lf.text, crlf.text);
    assert!(lf.text.ends_with("}\n"));
    // Lines count a CRLF once.
    let dup = json("{\"a\":1,\r\n\"a\":2}").unwrap();
    assert_eq!(
        dup.notes,
        vec![SortNote::DuplicateKey {
            key: "\"a\"".into(),
            line: 2
        }]
    );
}

#[test]
fn a_side_over_the_limit_is_left_alone() {
    let big = "[".repeat(SORT_LIMIT + 1);
    assert_eq!(
        sort_keys(&big, SortKind::Json, &CancelToken::new()),
        Err(SortError::TooLarge)
    );
    assert_eq!(
        sort_keys(&big, SortKind::Yaml, &CancelToken::new()),
        Err(SortError::TooLarge)
    );
}

#[test]
fn nesting_past_256_levels_is_refused() {
    let deep = |n: usize| format!("{}{}", "[".repeat(n), "]".repeat(n));
    assert!(json(&deep(256)).is_ok());
    assert_eq!(json(&deep(300)), Err(SortError::TooDeep));
    assert_eq!(json(&"[".repeat(300)), Err(SortError::TooDeep));
}

#[test]
fn a_cancelled_token_stops_the_sort() {
    let cancel = CancelToken::new();
    cancel.cancel();
    assert_eq!(
        sort_keys("{\"a\":1}", SortKind::Json, &cancel),
        Err(SortError::Cancelled)
    );
    assert_eq!(
        sort_keys("a: 1", SortKind::Yaml, &cancel),
        Err(SortError::Cancelled)
    );
}

#[test]
fn too_many_nodes_are_refused() {
    let many = format!("[{}0]", "0,".repeat(MAX_NODES));
    assert_eq!(json(&many), Err(SortError::TooBig));
}

#[test]
fn sort_kind_for_uses_the_extension_then_the_content() {
    let kind = |path: &str, text: &str| sort_kind_for(Path::new(path), text);
    assert_eq!(kind("a.json", "x"), Some(SortKind::Json));
    assert_eq!(kind("a.GEOJSON", "x"), Some(SortKind::Json));
    assert_eq!(kind("a.yml", "x"), Some(SortKind::Yaml));
    assert_eq!(kind("a.yaml", "x"), Some(SortKind::Yaml));
    assert_eq!(kind("", "{\"a\":1}"), Some(SortKind::Json));
    assert_eq!(kind("notes.txt", "  [1]"), Some(SortKind::Json));
    assert_eq!(kind("notes.txt", "\u{feff}{}"), Some(SortKind::Json));
    assert_eq!(kind("notes.txt", "a: 1"), None);
    assert_eq!(kind("a.yaml", "{\"b\": 1}"), Some(SortKind::Yaml));
    assert_eq!(kind("a.jsonc", "{\"a\":1}"), Some(SortKind::Json));
}

mod props {
    use super::*;
    use proptest::prelude::*;
    use std::collections::BTreeMap;

    #[derive(Debug, Clone)]
    enum Value {
        Scalar(&'static str),
        Array(Vec<Value>),
        Object(BTreeMap<String, Value>),
    }

    fn write(value: &Value, out: &mut String) {
        match value {
            Value::Scalar(text) => out.push_str(text),
            Value::Array(items) => {
                out.push('[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    write(item, out);
                }
                out.push(']');
            }
            Value::Object(map) => {
                out.push('{');
                for (i, (key, item)) in map.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    out.push_str(&format!("\"{key}\":"));
                    write(item, out);
                }
                out.push('}');
            }
        }
    }

    fn value() -> impl Strategy<Value = Value> {
        let leaf = prop::sample::select(vec!["1", "1.50", "-0", "true", "null", "\"x y\""])
            .prop_map(Value::Scalar);
        leaf.prop_recursive(4, 24, 4, |inner| {
            prop_oneof![
                prop::collection::vec(inner.clone(), 0..4).prop_map(Value::Array),
                prop::collection::btree_map("[a-z]{1,3}", inner, 0..4).prop_map(Value::Object),
            ]
        })
    }

    proptest! {
        #[test]
        fn sorted_input_matches_format_json_byte_for_byte(doc in value(), newline in any::<bool>()) {
            let mut text = String::new();
            write(&doc, &mut text);
            if newline {
                text.push('\n');
            }
            prop_assert_eq!(Some(json_text(&text)), format_json(&text));
        }
    }
}

#[test]
fn a_final_cr_counts_as_a_final_break() {
    assert_eq!(json_text("{\"a\":1}\r"), json_text("{\"a\":1}\n"));
    assert!(json_text("{\"a\":1}\r").ends_with('\n'));
}

#[test]
fn a_key_written_twice_is_noted() {
    let sorted = json(r#"{"a":1,"a":2}"#).unwrap();
    assert_eq!(sorted.text, "{\n  \"a\": 1,\n  \"a\": 2\n}");
    assert_eq!(
        sorted.notes,
        vec![SortNote::DuplicateKey {
            key: "\"a\"".into(),
            line: 1
        }]
    );
}

#[test]
fn a_small_file_cannot_print_past_the_output_limit() {
    // 400 000 items at 100 levels of indentation: 0.8 MB of text, over 80 MB printed.
    let text = format!(
        "{}{}0{}",
        "[".repeat(100),
        "0,".repeat(400_000),
        "]".repeat(100)
    );
    assert!(text.len() < 1_000_000);
    assert!(matches!(json(&text), Err(SortError::TooBig)));
}

#[test]
fn a_final_cr_with_crlf_breaks_matches_lf() {
    let crlf = json("{\r\n  \"b\": 1,\r\n  \"a\": 2\r\n}\r").unwrap();
    let lf = json("{\n  \"b\": 1,\n  \"a\": 2\n}\n").unwrap();
    assert_eq!(crlf.text, lf.text);
}
