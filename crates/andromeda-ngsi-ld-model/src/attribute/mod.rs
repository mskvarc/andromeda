//! The NGSI-LD attribute families and the members they share (ETSI GS CIM 009 clause 5.2).

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod budgeted_member_tests;
pub mod common_fields;
#[cfg(test)]
#[expect(
    clippy::expect_used,
    clippy::panic,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod family_member_prohibition_tests;
pub mod geo_property;
pub mod geo_property_value;
pub mod inline_entity;
pub mod instance;
pub(crate) mod instance_member;
mod instance_node;
pub mod json_property;
pub mod key_screening;
pub mod language_property;
pub mod list_property;
pub mod list_relationship;
pub mod member_decoding;
mod member_node;
pub mod member_reading;
#[cfg(test)]
#[expect(
    clippy::expect_used,
    clippy::panic,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod member_reading_tests;
pub mod merge_instance;
pub mod merge_member;
pub mod merge_value;
pub mod nested_null;
pub mod null_value;
pub mod object_list;
pub mod one_or_many;
pub mod patched;
pub mod previous_instance;
pub mod property;
pub mod recorded_elements;
pub mod relationship;
pub(crate) mod structure_budget;
pub mod value;
pub mod vocab_property;
pub mod written_attribute;
