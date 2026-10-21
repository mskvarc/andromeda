//! A JSON primitive value, borrowed, as the datatype checks read it.

use andromeda_ngsi_ld_number::JsonNumber;

/// One JSON primitive a literal holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrimitiveLiteral<'a> {
    /// A JSON string.
    String(&'a str),
    /// A JSON number, at its exact value.
    Number(&'a JsonNumber),
    /// A JSON boolean.
    Boolean(bool),
}
