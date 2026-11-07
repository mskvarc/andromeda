//! The JSON-LD 1.1 expansion algorithm over a compact JSON document, for the
//! shapes NGSI-LD requests take.
//!
//! Each step follows the algorithm of the JSON-LD 1.1 Processing Algorithms
//! and API (section 5.1.2) and the processor this crate falls back to, step
//! for step, deciding every term with the processed context: an element
//! ([`element`]), an object ([`object`]), a value object ([`value_object`]), a
//! scalar ([`literal`]) and the value of one member ([`member`]). Whatever the
//! algorithm would do that these steps do not model, they decline.

pub(crate) mod element;
pub(crate) mod literal;
pub(crate) mod member;
pub(crate) mod object;
pub(crate) mod value_object;
