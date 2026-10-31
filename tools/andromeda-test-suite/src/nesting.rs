//! Documents nested to a chosen depth: the object, list and sub-attribute chains the depth limit tests write, the same
//! chains as JSON text for a request the model cannot write, and the matcher that reads a sub-attribute chain back.

use crate::{
    attribute_matcher::{AttributeMatcher, property},
    value::object,
};
use ngsi_ld::model::{BuildError, Input, Property, Value};

/// The object value `{"k": {"k": ... "leaf"}}` with `levels` objects around `leaf`.
#[must_use]
pub fn object_chain(levels: usize, leaf: &str) -> Value {
    (0..levels).fold(Value::from(leaf), |inner, _| object([("k", inner)]))
}

/// The array `[[ ... [1] ... ]]` of `levels` arrays around the number 1.
#[must_use]
pub fn list_chain(levels: usize) -> Value {
    (0..levels).fold(Value::from(1), |inner, _| Value::Array(vec![inner]))
}

/// The JSON text of [`object_chain`], for a body the model does not write.
#[must_use]
pub fn object_chain_text(levels: usize, leaf: &str) -> String {
    format!("{}\"{leaf}\"{}", "{\"k\":".repeat(levels), "}".repeat(levels))
}

/// A Property of value 1 carrying `levels` nested sub-attributes named `s`, each a Property of value 1 (so the chain
/// `s.s.s...` is `levels` long and the last `s` carries none).
///
/// # Errors
///
/// Returns [`BuildError`] when a Property of the chain is refused by its builder.
pub fn sub_attribute_chain(levels: usize) -> Result<Property<Input>, BuildError> {
    (0..levels).try_fold(Property::builder(1).build()?, |inner, _| Property::builder(1).property("s", inner).build())
}

/// The attribute read back from [`sub_attribute_chain`]`(levels)`: a Property of value 1 whose sub-attribute `s` is
/// again such a Property, `levels` times, the last one with no `s` of its own.
#[must_use]
pub fn sub_attribute_chain_matcher(levels: usize) -> AttributeMatcher {
    (0..levels).fold(property().value(1).without_sub_attribute("s"), |inner, _| {
        property().value(1).sub_attribute("s", inner)
    })
}

#[cfg(test)]
mod tests {
    use crate::nesting::{list_chain, object_chain, object_chain_text, sub_attribute_chain};
    use googletest::prelude::*;
    use ngsi_ld::model::Value;

    #[gtest]
    fn an_object_chain_nests_one_object_per_level() {
        let chain = object_chain(2, "leaf");
        let Value::Object(members) = &chain else {
            return fail!("not an object").and_log_failure();
        };
        expect_that!(members.keys().collect::<Vec<_>>(), elements_are![eq(&"k")]);
    }

    #[gtest]
    fn the_text_of_a_chain_has_balanced_braces() {
        expect_that!(object_chain_text(3, "x"), eq(r#"{"k":{"k":{"k":"x"}}}"#));
        expect_that!(object_chain_text(0, "x"), eq(r#""x""#));
    }

    #[gtest]
    fn a_list_chain_nests_one_array_per_level() {
        expect_that!(list_chain(2), eq(&Value::Array(vec![Value::Array(vec![Value::from(1)])])));
    }

    #[gtest]
    fn a_sub_attribute_chain_of_any_length_builds() -> Result<()> {
        expect_that!(sub_attribute_chain(0).is_ok(), eq(true));
        verify_that!(sub_attribute_chain(32).is_ok(), eq(true))
    }
}
