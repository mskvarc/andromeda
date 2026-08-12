//! Error types for the temporal store crate.

use crate::{duplicate_instance_claim::DuplicateInstanceClaimError, unit_code_metadata::UnitCodeMetadataError};
use andromeda_ngsi_ld_lexicon::ngsi_ld_date_time::NgsiLdDateTimeError;
use andromeda_ngsi_ld_model::attribute::json_number::JsonNumberError;
use andromeda_postgresql_encoding::ewkb::EwkbError;
use andromeda_postgresql_temporal_query::aggregation_group::MixedAggregationGroup;
use andromeda_temporal::error::TemporalError;

/// Errors produced by a temporal store implementation.
///
/// Internal to the PostgreSQL adapter boundary: the direct port implementation converts these into [`TemporalError`] without exposing store detail to policy or HTTP clients.
#[derive(Debug, thiserror::Error)]
pub enum TemporalStoreError {
    /// The underlying database query failed.
    #[error("temporal store: {0}")]
    Database(#[from] sqlx::Error),
    /// A stored value could not be decoded into a domain type.
    #[error("temporal store decode: {0}")]
    Decode(String),
    /// The store could not build or run the requested query.
    #[error("temporal store query: {0}")]
    Query(String),
    /// The requested entity or attribute instance was not found.
    #[error("temporal store not found: {0}")]
    NotFound(String),
    /// A geometry value could not be encoded for persistence.
    #[error("temporal store encoding: {0}")]
    Encoding(#[from] EwkbError),
    /// An attribute name could not be resolved in the dictionary.
    #[error("temporal store attribute lookup: {0}")]
    AttributeLookup(String),
    /// An extracted attribute value was invalid.
    #[error("temporal store validation: {0}")]
    Validation(String),
    /// A numeric value could not be carried in the shape JSON gives it.
    ///
    /// The history value column is `NUMERIC` and takes the value's decimal
    /// literal, so magnitude alone no longer refuses a write. What remains
    /// refusable is a number JSON has no literal for and a stored literal that
    /// reads back as none of the three shapes a JSON number can take.
    #[error("temporal store numeric precision: {0}")]
    NumericPrecision(#[from] JsonNumberError),
    /// A DateTime attribute value could not be accepted at microsecond resolution.
    #[error("temporal store timestamp precision: {0}")]
    TimestampPrecision(#[from] NgsiLdDateTimeError),
    /// The instance's `unitCode` member could not be moved to or from its column.
    #[error("temporal store unit code: {0}")]
    UnitCode(#[from] UnitCodeMetadataError),
    /// The requested aggregate would have spanned inputs that do not share a meaning.
    #[error("temporal store aggregation: {0}")]
    MixedAggregationGroup(#[from] MixedAggregationGroup),
    /// A client-supplied `instanceId` claimed more than one attribute instance
    /// of the same attribute at once.
    #[error("temporal store instance claim: {0}")]
    DuplicateInstanceClaim(#[from] DuplicateInstanceClaimError),
}

impl From<TemporalStoreError> for TemporalError {
    fn from(error: TemporalStoreError) -> TemporalError {
        match error {
            TemporalStoreError::NotFound(detail) => TemporalError::not_found(detail),
            TemporalStoreError::Validation(detail) => TemporalError::validation(detail),
            // Both precision refusals are about the value the client sent, not
            // about the store, so they surface as validation failures rather
            // than as persistence faults.
            TemporalStoreError::NumericPrecision(_) | TemporalStoreError::TimestampPrecision(_) => TemporalError::validation(error.to_string()),
            // A unit the client sent is a request fault; an identity already in
            // a row that the catalog no longer registers is catalog drift.
            TemporalStoreError::UnitCode(detail) => match detail {
                UnitCodeMetadataError::Malformed | UnitCodeMetadataError::Unregistered { .. } => TemporalError::validation(detail.to_string()),
                UnitCodeMetadataError::UnknownIdentity { .. } | UnitCodeMetadataError::Catalog(_) => TemporalError::persistence(detail.to_string()),
            },
            // Which units, families and value shapes the selected instances
            // carry is a property of the request, so an aggregate spanning
            // several of them is refused as client input.
            TemporalStoreError::MixedAggregationGroup(_) => TemporalError::validation(error.to_string()),
            // Two instances claiming the same instanceId is a conflict in the
            // client's own write, not a broker malfunction (clause 4.5.7).
            TemporalStoreError::DuplicateInstanceClaim(_) => TemporalError::validation(error.to_string()),
            TemporalStoreError::Database(_)
            | TemporalStoreError::Decode(_)
            | TemporalStoreError::Query(_)
            | TemporalStoreError::Encoding(_)
            | TemporalStoreError::AttributeLookup(_) => TemporalError::persistence(error.to_string()),
        }
    }
}
