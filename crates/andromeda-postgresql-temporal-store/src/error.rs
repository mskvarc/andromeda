//! Error types for the temporal store crate.

use crate::{duplicate_instance_claim::DuplicateInstanceClaimError, unit_code_metadata::UnitCodeMetadataError};
use andromeda_ngsi_ld_lexicon::ngsi_ld_date_time::NgsiLdDateTimeError;
use andromeda_postgresql_collation::provisioned_collations::CollationProvisioningError;
use andromeda_postgresql_encoding::{copy::CopyFieldError, ewkb::EwkbError};
use andromeda_postgresql_temporal_query::aggregation_group::MixedAggregationGroup;
use andromeda_temporal::{error::TemporalError, temporal_not_found_reason::TemporalNotFoundReason, temporal_validation_reason::TemporalValidationReason};

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
    /// An ICU collation a clause-4.23 ordering named could not be created.
    #[error("temporal store collation: {0}")]
    Collation(#[from] CollationProvisioningError),
    /// A value could not be staged as a binary COPY field: it is longer than
    /// the format's 32-bit field prefix can announce, or it does not
    /// serialise.
    #[error("temporal store COPY field: {0}")]
    CopyField(#[from] CopyFieldError),
    /// An attribute name could not be resolved in the dictionary.
    #[error("temporal store attribute lookup: {0}")]
    NameLookup(String),
    /// An extracted attribute value was invalid.
    #[error("temporal store validation: {0}")]
    Validation(String),
    /// A `DateTime` attribute value could not be accepted at microsecond resolution.
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
            TemporalStoreError::NotFound(detail) => TemporalError::NotFound(TemporalNotFoundReason::Store { detail }),
            TemporalStoreError::Validation(detail) => TemporalError::Validation(TemporalValidationReason::Store { detail }),
            // A unit the client sent is a request fault; an identity already in
            // a row that the catalog no longer registers is catalog drift.
            TemporalStoreError::UnitCode(detail) => match detail {
                UnitCodeMetadataError::Malformed | UnitCodeMetadataError::Unregistered { .. } => {
                    TemporalError::Validation(TemporalValidationReason::Store { detail: detail.to_string() })
                }
                UnitCodeMetadataError::UnknownIdentity { .. } | UnitCodeMetadataError::Catalog(_) => TemporalError::persistence(detail.to_string()),
            },
            // Each of these three is a fault in the client's own request rather
            // than a broker malfunction: a timestamp at a resolution the store
            // cannot hold is client input; which units, families and value
            // shapes the selected instances carry is a property of the request,
            // so an aggregate spanning several of them is refused as client
            // input too; and two instances claiming the same instanceId is a
            // conflict inside the submitted write (clause 4.5.7).
            TemporalStoreError::TimestampPrecision(_) | TemporalStoreError::MixedAggregationGroup(_) | TemporalStoreError::DuplicateInstanceClaim(_) => {
                TemporalError::Validation(TemporalValidationReason::Store { detail: error.to_string() })
            }
            // Clause 5.7.4.4: "if a preferred collation setting is present and
            // it does not conform to a valid ICU collation (see IETF RFC 6067)
            // then an error of type BadRequestData shall be raised". Whether
            // ICU holds a collation for a locale is a question only the
            // database can answer, so its refusal of the client's own
            // `collation` reaches policy here rather than as a broker fault.
            TemporalStoreError::Collation(CollationProvisioningError::NotAnIcuCollation { collation, .. }) => {
                TemporalError::Validation(TemporalValidationReason::Collation {
                    collation: collation.to_string(),
                })
            }
            TemporalStoreError::Database(_)
            | TemporalStoreError::Decode(_)
            | TemporalStoreError::Query(_)
            | TemporalStoreError::Encoding(_)
            | TemporalStoreError::CopyField(_)
            | TemporalStoreError::Collation(CollationProvisioningError::Statement { .. })
            | TemporalStoreError::NameLookup(_) => TemporalError::persistence(error.to_string()),
        }
    }
}
