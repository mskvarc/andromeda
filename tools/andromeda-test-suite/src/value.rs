//! Building NGSI-LD values a test writes inline.

use indexmap::IndexMap;
use ngsi_ld::model::Value;

/// The object value with `members`, in their order (clause 4.5.2.2 keeps it as written).
#[must_use]
pub fn object<const N: usize>(members: [(&str, Value); N]) -> Value {
    Value::Object(members.into_iter().map(|(name, value)| (name.to_owned(), value)).collect::<IndexMap<_, _>>())
}

/// The value a JSON tree read from a document the model carries as opaque JSON (a `GeoJSON` foreign member) stands
/// for, so it is compared as any value is; `None` for the JSON `null`, which no NGSI-LD value writes.
#[must_use]
pub fn from_json(json: &serde_json::Value) -> Option<Value> {
    Some(match json {
        serde_json::Value::Null => return None,
        serde_json::Value::Bool(flag) => Value::Boolean(*flag),
        serde_json::Value::Number(number) => Value::Number(number.to_string().parse().ok()?),
        serde_json::Value::String(text) => Value::from(text.as_str()),
        serde_json::Value::Array(items) => Value::Array(items.iter().map(from_json).collect::<Option<Vec<_>>>()?),
        serde_json::Value::Object(members) => Value::Object(
            members
                .iter()
                .map(|(name, member)| from_json(member).map(|value| (name.clone(), value)))
                .collect::<Option<IndexMap<_, _>>>()?,
        ),
    })
}

#[cfg(test)]
mod tests {
    use crate::value::{from_json, object};
    use googletest::prelude::*;
    use ngsi_ld::model::Value;

    #[gtest]
    fn an_object_keeps_its_members_in_order() {
        let value = object([("b", Value::from(1)), ("a", Value::from("x"))]);
        let Value::Object(members) = &value else {
            return fail!("not an object").and_log_failure();
        };
        expect_that!(members.keys().collect::<Vec<_>>(), elements_are![eq(&"b"), eq(&"a")]);
    }

    #[gtest]
    fn a_json_tree_reads_as_the_value_it_writes() -> Result<()> {
        let json: serde_json::Value = serde_json::from_str(r#"{"year":2024,"marks":[1,2],"ok":true}"#)?;
        let value = from_json(&json).or_fail()?;
        verify_that!(
            value,
            eq(&object([
                ("year", Value::from(2024)),
                ("marks", Value::from(vec![1, 2])),
                ("ok", Value::Boolean(true))
            ]))
        )
    }
}
