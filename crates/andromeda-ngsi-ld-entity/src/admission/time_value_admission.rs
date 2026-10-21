//! Holding every value a written document declares as a clause 4.6.3 time
//! value to its declared form, at every depth.
//!
//! Clause 4.6.3 fixes each of the `DateTime`, `Date` and `Time` forms, and
//! annex C.6 makes the `@value`/`@type` pair the way a value says it is one.
//! A sub-attribute is an attribute of its instance (clause 4.5.2.2) and a
//! structured value nests values of its own, so the rule holds wherever a
//! value stands, on the Core API and the Temporal API alike.

use crate::{
    document::{
        EntityDocument,
        instance_document::InstanceDocument,
        written_value::{ValueSlot, WrittenValue},
    },
    expanded_value::ExpandedValue,
    literal::LiteralValue,
    refusal::EntityRefusal,
    value_mode::{Fragment, FragmentMember},
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, time::value::TimeValue};

impl EntityDocument {
    /// Hold every value of the document declared as a clause 4.6.3 time value
    /// to its declared form, at every depth.
    ///
    /// # Errors
    ///
    /// Returns [`EntityRefusal::TimeValue`] for the first value that is not of
    /// its declared form.
    pub fn admit_time_values(&self) -> Result<(), EntityRefusal> {
        self.attributes()
            .iter()
            .try_for_each(|(name, member)| member.instances().iter().try_for_each(|instance| admit_time_values(name, instance)))
    }
}

impl InstanceDocument {
    /// Hold every value of the instance written for `attribute` declared as a
    /// clause 4.6.3 time value to its declared form, at every depth.
    ///
    /// # Errors
    ///
    /// Returns [`EntityRefusal::TimeValue`] for the first value that is not of
    /// its declared form.
    pub fn admit_time_values(&self, attribute: &AttributeName) -> Result<(), EntityRefusal> {
        admit_time_values(attribute, self)
    }
}

/// Every value declared as a `DateTime`, a `Date` or a `Time`, here or on any
/// sub-attribute below it and at any depth of a structured value, is one
/// (clause 4.6.3).
///
/// Clause 4.6.3 fixes each form completely, and annex C.6 makes the
/// `@value`/`@type` pair the way a value says it is one, so a value declared
/// as a `DateTime` and written `2024-01-01T00:00:00+01:00` or `yesterday` is a
/// malformed value of its declared type, not a string. A value that declares
/// no form is a time value only when it is written in one and satisfies it
/// ([`TimeValue::admitted`]), which never refuses.
fn admit_time_values(name: &AttributeName, instance: &InstanceDocument) -> Result<(), EntityRefusal> {
    let values = match instance.value() {
        ValueSlot::Set(WrittenValue::Value(values) | WrittenValue::ValueList(values)) => values.as_slice(),
        ValueSlot::Set(
            WrittenValue::Json(_)
            | WrittenValue::Geometry(_)
            | WrittenValue::Object(_)
            | WrittenValue::LanguageMap(_)
            | WrittenValue::Vocab(_)
            | WrittenValue::ObjectList(_),
        )
        | ValueSlot::Absent
        | ValueSlot::Remove(_) => &[],
    };
    values.iter().try_for_each(|value| admit_time_value(name, value))?;
    instance
        .sub_attributes()
        .iter()
        .try_for_each(|(sub_name, member)| member.instances().iter().try_for_each(|sub| admit_time_values(sub_name, sub)))
}

/// One value of the attribute `name` and everything inside it, under
/// [`admit_time_values`].
fn admit_time_value(name: &AttributeName, value: &ExpandedValue<Fragment>) -> Result<(), EntityRefusal> {
    match value {
        ExpandedValue::Literal(literal) => match (literal.value(), literal.datatype()) {
            (LiteralValue::String(text), Some(datatype)) => match TimeValue::admitted(Some(datatype.as_str()), text) {
                Some(Err(source)) => Err(EntityRefusal::TimeValue {
                    // clone: the refusal outlives the borrowed request.
                    attribute: name.clone(),
                    source,
                }),
                Some(Ok(_)) | None => Ok(()),
            },
            (LiteralValue::String(_), None) | (LiteralValue::Number(_) | LiteralValue::Boolean(_), Some(_) | None) => Ok(()),
        },
        ExpandedValue::Json(_) => Ok(()),
        ExpandedValue::List(items) => items.iter().try_for_each(|item| admit_time_value(name, item)),
        ExpandedValue::Node(node) => node.members().values().try_for_each(|member| match member {
            FragmentMember::Remove => Ok(()),
            FragmentMember::Values(values) => values.iter().try_for_each(|item| admit_time_value(name, item)),
        }),
    }
}
