//! The NGSI-LD attribute families and the members they share (ETSI GS CIM 009 clause 5.2).

mod canonical_numbers;
pub mod common_fields;
pub mod geo_property;
pub mod inline_entity;
pub mod instance;
mod instance_member;
mod instance_node;
pub mod instance_revalidation;
pub mod json_property;
pub mod language_property;
pub mod list_property;
pub mod list_relationship;
pub mod member_decoding;
mod member_node;
#[cfg(test)]
mod member_reading_tests;
pub mod object_list;
pub mod one_or_many;
pub mod property;
pub mod relationship;
pub mod value;
pub mod vocab_property;
