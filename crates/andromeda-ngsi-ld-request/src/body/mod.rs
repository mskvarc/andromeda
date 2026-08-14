//! The `POST` bodies that stand in for a query string.
//!
//! Clause 6.23 (POST query) defines the `POST` query operations by their equivalence to
//! the `GET` ones, so each body here converts into the parameter set of the
//! query it stands for rather than being interpreted separately.

pub mod entity_map;
