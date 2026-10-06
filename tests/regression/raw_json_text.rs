//! Reading the member order of a JSON response from its raw text.
//!
//! A parsed JSON map does not keep member order, so a test that asserts which
//! member a response body opens with has to look at the bytes. This module
//! splits a top-level JSON array into the raw text of its items and says whether
//! one object's text opens with `@context`; it needs no broker.

/// Whether `object` is the text of a JSON object whose first member is
/// `@context`, with whitespace allowed before the brace and around the key.
pub fn opens_with_context(object: &str) -> bool {
    object
        .trim_start()
        .strip_prefix('{')
        .is_some_and(|rest| rest.trim_start().starts_with("\"@context\""))
}

/// The raw text of every item of the JSON array `text`, in document order.
///
/// Returns `None` when `text` is not a JSON array. Strings are skipped as a
/// unit, so braces, brackets and commas inside them never split an item.
pub fn array_items(text: &str) -> Option<Vec<&str>> {
    let trimmed = text.trim();
    let inner = trimmed.strip_prefix('[')?.strip_suffix(']')?;
    let mut items = Vec::new();
    let mut depth = 0_usize;
    let mut in_string = false;
    let mut escaped = false;
    let mut start = 0_usize;
    for (offset, character) in inner.char_indices() {
        if in_string {
            match (escaped, character) {
                (true, _) => escaped = false,
                (false, '\\') => escaped = true,
                (false, '"') => in_string = false,
                (false, _) => {}
            }
            continue;
        }
        match character {
            '"' => in_string = true,
            '{' | '[' => depth += 1,
            '}' | ']' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                items.push(inner[start..offset].trim());
                start = offset + 1;
            }
            _ => {}
        }
    }
    let last = inner[start..].trim();
    if !last.is_empty() {
        items.push(last);
    }
    Some(items)
}

#[cfg(test)]
mod tests {
    use super::{array_items, opens_with_context};

    #[test]
    fn opens_with_context_accepts_a_leading_context_member_after_whitespace() {
        assert!(opens_with_context("  {\n  \"@context\": [], \"id\": \"a\"}"));
    }

    #[test]
    fn opens_with_context_refuses_an_object_whose_context_member_comes_later() {
        assert!(!opens_with_context(r#"{"id": "a", "@context": []}"#));
    }

    #[test]
    fn array_items_splits_on_top_level_commas_only() {
        let items = array_items(r#"[{"a": [1, 2], "b": "x,}"}, {"c": "q\",{"}]"#).expect("an array");

        pretty_assertions::assert_eq!(items, vec![r#"{"a": [1, 2], "b": "x,}"}"#, r#"{"c": "q\",{"}"#]);
    }

    #[test]
    fn array_items_answers_none_for_an_object() {
        assert!(array_items(r#"{"a": 1}"#).is_none());
    }

    #[test]
    fn array_items_answers_no_items_for_an_empty_array() {
        pretty_assertions::assert_eq!(array_items("[]"), Some(Vec::new()));
    }
}
