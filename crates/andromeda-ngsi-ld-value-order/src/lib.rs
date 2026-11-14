//! The order relations ETSI GS CIM 009 v1.9.1 clause 4.9 compares values by.
//!
//! A Query Term compares a target value with its operand in an order the
//! clause fixes per data type: numbers as exact decimals, with the XSD
//! infinities and not-a-number beside them, and text strings by code point for
//! equality and under the Unicode Collation Algorithm for ordering. Each
//! relation is stated once here, so every execution of the language that orders
//! values outside a store orders them alike.

pub mod number_order;
pub mod string_order;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-ngsi-ld-number",
    ],
    allow_workspace_dev_dependencies: [
    ],
    allow_production_dependencies: [],
}

#[cfg(test)]
andromeda_architecture_checks::forbid_sql!();
