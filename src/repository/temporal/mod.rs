//! Persistence layer for the NGSI-LD Temporal API.

/// Reading the parts of one expanded attribute instance.
pub mod attribute_instance;
/// The value an attribute instance carries, and the column it lands in.
pub mod attribute_value;
pub mod copy;
pub mod entry;
/// Decoding `PgRow` into the backend-neutral temporal row types.
pub mod history_row_decode;
pub mod mapper;
/// [`TemporalMutator`] implementation on [`TemporalRepository`].
pub mod mutator;
/// Turning expanded entity payloads into the rows one temporal write commits.
pub mod observation;
pub mod params;
/// [`TemporalHistoryReader`] implementation on [`TemporalRepository`].
pub mod reader;
/// Writing attribute history whenever entities are created or modified.
pub mod recorder;
pub mod repo;
pub mod rows;
pub mod value_type;
