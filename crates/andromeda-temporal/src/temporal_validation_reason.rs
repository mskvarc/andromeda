//! The concrete reasons a temporal request or representation is rejected as
//! `BadRequestData`.
//!
//! Every client-facing validation failure of temporal policy is one variant
//! here, carrying the offending value as a typed field and chaining the typed
//! source error where one exists, so
//! [`TemporalError::Validation`](crate::error::TemporalError::Validation) never
//! holds an ad-hoc string. Each `#[error]` renders lowercase client prose; spec
//! attribute-type names (`Property`, `GeoProperty`, `Relationship`, ...) keep
//! their fixed casing, and clause numbers stay in doc comments.

use crate::aggregation_conflict::AggregationConflict;
use andromeda_context_commit::submission_refusal::SubmissionRefusal;
use andromeda_ngsi_ld_entity::{operation::mutation_error::MutationError, refusal::EntityRefusal};
use andromeda_ngsi_ld_lexicon::{
    attribute_name::AttributeName,
    attribute_type::AttributeType,
    reference::NgsiLdReferenceError,
    unit_code_resolution::UnitCodeRefusal,
};
use andromeda_ngsi_ld_query::ordering::collation::IcuCollation;
use thiserror::Error;

/// Why a temporal request or representation is invalid.
#[derive(Debug, Error)]
pub enum TemporalValidationReason {
    /// The temporal entity payload omitted its `id`.
    #[error("entity is missing an id")]
    EntityMissingId,
    /// The temporal entity payload omitted its `type`.
    #[error("entity is missing a type")]
    EntityMissingType,
    /// An attribute name did not satisfy the attribute-name grammar.
    #[error("attribute name is invalid: {source}")]
    AttributeName {
        /// The grammar violation.
        #[source]
        source: NgsiLdReferenceError,
    },
    /// An instance of a unitless attribute family carried `unitCode` (clauses
    /// 4.5.3, 4.5.18, 4.5.20, 4.5.24).
    #[error("{family} '{attribute}' cannot carry 'unitCode': this attribute family is defined without one")]
    InstanceUnitCodeForbidden {
        /// The unitless attribute family.
        family: AttributeType,
        /// The offending attribute, by its expanded name.
        attribute: AttributeName,
    },
    /// A `unitCode` names no UN/CEFACT common code, or is not one plain
    /// string (Table 5.2.5-1, clause 4.5.2.2).
    #[error("'{attribute}' carries an unusable 'unitCode': {source}")]
    InstanceUnitCodeUnusable {
        /// The offending attribute, by its expanded name.
        attribute: AttributeName,
        /// Why the `unitCode` is not usable.
        #[source]
        source: UnitCodeRefusal,
    },
    /// The client's `collation` is not a locale the database can honour.
    #[error("'{collation}' does not conform to a valid ICU collation")]
    Collation {
        /// The rejected collation identifier.
        collation: IcuCollation,
    },
    /// The instances one aggregation group spans disagree in meaning, so a
    /// value-sensitive aggregate over them would compare incomparable values.
    #[error("attribute '{attribute}' aggregates over {conflict}")]
    MixedAggregation {
        /// The aggregated attribute.
        attribute: AttributeName,
        /// How its instances disagree.
        conflict: AggregationConflict,
    },
    /// The clause 5.6.14.3 `EntityTemporal` Fragment of an instance modification
    /// does not carry exactly one instance of the Attribute the request targets.
    #[error("the EntityTemporal Fragment does not carry one instance of the target attribute '{target}'")]
    FragmentWithoutTargetAttribute {
        /// The targeted attribute, by its expanded name.
        target: AttributeName,
    },
    /// The clause 5.6.14.3 replacement instance records no value: its value
    /// member is its family's deletion encoding, or it names neither a family
    /// nor a value.
    #[error("the replacement instance of '{target}' records no value")]
    ReplacementRecordsNoValue {
        /// The targeted attribute, by its expanded name.
        target: AttributeName,
    },
    /// The written document is not one the model admits (clauses 5.5.4 and
    /// 4.5): malformed, a geometry clause 4.7 gives no reading of, a member a
    /// family never carries, an unusable value.
    #[error(transparent)]
    Representation(EntityRefusal),
    /// A standing instance of a Temporal Evolution cannot be current state.
    #[error(transparent)]
    Mutation(MutationError),
    /// A compact request document could not be written for the JSON-LD
    /// processor.
    #[error("the request document cannot be expanded: {0}")]
    CompactDocument(#[source] serde_json::Error),
    /// The submitted content cannot be stored as written (clause 5.5.6).
    #[error(transparent)]
    Refused(#[from] SubmissionRefusal),
}
