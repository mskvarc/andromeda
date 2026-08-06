//! Persistence layer for the NGSI-LD Temporal API.

/// Reading the parts of one expanded attribute instance.
pub mod attribute_instance;
/// The value an attribute instance carries, and the column it lands in.
pub mod attribute_value;
pub mod copy;
pub mod entry;
pub mod mapper;
/// Turning expanded entity payloads into the rows one temporal write commits.
pub mod observation;
pub mod params;
/// Writing attribute history whenever entities are created or modified.
pub mod recorder;
pub mod repo;
pub mod rows;
pub mod value_type;
