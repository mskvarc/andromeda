//! Building NGSI-LD values a test writes inline.

use indexmap::IndexMap;
use ngsi_ld::model::{Datatype, NameError, TypedValue, TypedValueError, Value, VocabTerm};
use thiserror::Error;

/// Why an XSD-typed value could not be built.
#[derive(Debug, Error)]
pub enum XsdValueError {
    /// The datatype IRI is no vocabulary term.
    #[error("the XSD datatype is no term: {0}")]
    Datatype(#[from] NameError),
    /// The lexical form is refused.
    #[error("the lexical form is refused: {0}")]
    Lexical(#[from] TypedValueError),
}

/// The datatype `xsd:local` (XML Schema Definition Language 1.1 Part 2), by its full IRI.
///
/// # Errors
///
/// Returns [`XsdValueError::Datatype`] when `local` makes no IRI.
pub fn xsd(local: &str) -> Result<VocabTerm, XsdValueError> {
    Ok(format!("http://www.w3.org/2001/XMLSchema#{local}").parse()?)
}

/// The JSON-LD value object `{"@value": lexical, "@type": xsd:local}`.
///
/// # Errors
///
/// Returns [`XsdValueError`] when the datatype or the lexical form is refused.
pub fn xsd_typed(local: &str, lexical: &str) -> Result<Value, XsdValueError> {
    Ok(Value::Typed(TypedValue::try_new(Datatype::Other(xsd(local)?), lexical)?))
}

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
        serde_json::Value::Bool(flag) => Value::from(*flag),
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
    use crate::value::{from_json, object, xsd_typed};
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
                ("ok", Value::from(true))
            ]))
        )
    }

    #[gtest]
    fn an_xsd_typed_value_names_the_full_datatype_iri() -> Result<()> {
        let value = xsd_typed("integer", "4")?;
        let Value::Typed(typed) = &value else {
            return fail!("not a typed value");
        };
        expect_that!(typed.datatype().to_string(), eq("http://www.w3.org/2001/XMLSchema#integer"));
        expect_that!(typed.lexical(), eq("4"));
        Ok(())
    }
}
