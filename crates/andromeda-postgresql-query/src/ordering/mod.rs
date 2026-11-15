//! Ordering entities by the values of their attributes and by distance, the
//! clause 4.23 `orderBy` of a query, spelled as the sort keys of a statement.
//!
//! A term contributes the clause 4.23.2 rank of the datatype its value is,
//! then the value as that datatype compares (numbers and instants as
//! themselves, strings under the ICU root collation), or for a distance term
//! whether the attribute is a `GeoProperty` and then the distance.

pub mod attribute_value_datatype;
pub mod attribute_value_holder;
pub mod attribute_value_order;
pub mod order_geography;
pub mod result_ordering;
