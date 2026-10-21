//! The datatypes a value may be declared as, and whether a literal is a value of one.
//!
//! ETSI GS CIM 009 v1.9.1 clause 4.5.2 makes `valueType` "a string value which
//! shall be type coerced into a datatype URI" and asks that a JSON primitive
//! "align this with the RDF datatype". A JSON-LD `@type` on a value object
//! declares the same thing. RDF 1.1 Concepts section 5.3 makes a literal
//! ill-typed only when its datatype IRI is one the reader recognises, so the
//! broker holds a literal to its datatype exactly when the datatype is in the
//! recognised set ([`recognised`]): the three NGSI-LD time value data types and
//! the XSD datatypes RDF 1.1 section 5.1 lists as compatible. Any other IRI is
//! a valid datatype with nothing to check.

pub mod primitive_literal;
pub mod recognised;
