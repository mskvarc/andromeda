//! The exact comparison of an answer with the document Annex C prints: every member and every list item as printed,
//! lists in their printed order unless the specification leaves the order open, and the `@context` of an
//! `application/ld+json` answer required but not compared.

use googletest::prelude::*;
use serde_json::Value;

/// `document` without its `@context` member, failing when it has none.
fn without_context(mut document: Value) -> Result<Value> {
    document.as_object_mut().or_fail()?.remove("@context").or_fail()?;
    Ok(document)
}

/// Expects `body` to hold an `@context` member, whatever its value, and otherwise to be exactly `expected`.
pub(crate) fn expect_with_any_context(body: Value, expected: &Value) {
    expect_that!(body.get("@context"), some(anything()));
    let mut document = body;
    if let Some(members) = document.as_object_mut() {
        members.remove("@context");
    }
    expect_that!(&document, eq(expected));
}

/// Expects the flat array `body` to hold exactly the entities `expected` in any order, each with or without an
/// `@context` member.
pub(crate) fn expect_flat(body: Value, expected: Vec<Value>) -> Result<()> {
    let Value::Array(items) = body else {
        return fail!("the flattened answer is not an array: {body}");
    };
    let mut actual = items
        .into_iter()
        .map(|item| if item.get("@context").is_some() { without_context(item) } else { Ok(item) })
        .collect::<Result<Vec<_>>>()?;
    let mut expected = expected;
    sort_items_by(&mut actual, "id");
    sort_items_by(&mut expected, "id");
    expect_that!(&actual, eq(&expected));
    Ok(())
}

/// Orders the list at `pointer` in `document` by the text of member `key` of its items, for a list whose order the
/// specification leaves open; a missing list is left for the comparison to report.
pub(crate) fn sort_list_by(document: &mut Value, pointer: &str, key: &str) {
    if let Some(Value::Array(items)) = document.pointer_mut(pointer) {
        sort_items_by(items, key);
    }
}

/// Orders `items` by the text of their member `key`.
pub(crate) fn sort_items_by(items: &mut [Value], key: &str) {
    items.sort_by(|left, right| left.get(key).and_then(Value::as_str).cmp(&right.get(key).and_then(Value::as_str)));
}

/// `value` with every number written as a float, for a member whose numbers the annex prints as floats while a
/// broker may write a whole number without its fraction.
pub(crate) fn as_floats(value: &Value) -> Value {
    // The result is a new tree; the answer it is built from stays as received.
    match value {
        Value::Number(number) => number.as_f64().map_or_else(|| value.clone(), Value::from),
        Value::Array(items) => Value::Array(items.iter().map(as_floats).collect()),
        Value::Object(members) => Value::Object(members.iter().map(|(name, member)| (name.clone(), as_floats(member))).collect()),
        Value::Null | Value::Bool(_) | Value::String(_) => value.clone(),
    }
}

/// Orders the list of texts at `pointer` in `document`, for a list whose order the specification leaves open; a
/// missing list is left for the comparison to report.
pub(crate) fn sort_texts(document: &mut Value, pointer: &str) {
    if let Some(Value::Array(items)) = document.pointer_mut(pointer) {
        items.sort_by(|left, right| left.as_str().cmp(&right.as_str()));
    }
}

/// Removes member `name` from every item of the list at `pointer` in `document`, for a member an answer may carry
/// or not; a missing list is left for the comparison to report.
pub(crate) fn remove_from_items(document: &mut Value, pointer: &str, name: &str) {
    if let Some(Value::Array(items)) = document.pointer_mut(pointer) {
        for members in items.iter_mut().filter_map(Value::as_object_mut) {
            members.remove(name);
        }
    }
}

/// Expects the `@context` of `body` to name `reference` and nothing else: the reference alone, or a list holding
/// only it.
pub(crate) fn expect_context_only(body: &Value, reference: &str) {
    let named = Value::from(reference);
    expect_that!(
        body.get("@context"),
        some(any![
            eq(&named),
            predicate(|context: &Value| context
                .as_array()
                .is_some_and(|items| !items.is_empty() && items.iter().all(|item| item == &named)))
        ])
    );
}
