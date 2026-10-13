//! Error types for the temporal store crate.

use crate::{
    columnstore_enablement::StoredColumnstore,
    duplicate_instance_claim::DuplicateInstanceClaimError,
    history_members::HistoryMembersError,
    retention_sweep::TemporalRetentionHorizonInvalid,
    schema_ddl::timescale_policy::HypertableName,
    stored_aggregation_error::StoredAggregationError,
    stored_catalog_error::StoredCatalogError,
    stored_history_error::StoredHistoryError,
    temporal_store_invariant::TemporalStoreInvariant,
};
use andromeda_context_commit::{submission_refusal::SubmissionRefusal, write_id::WriteId, write_ordinal::WriteOrdinalOutOfRange};
use andromeda_ngsi_ld_entity::unit_code::StoredUnitCodeError;
use andromeda_ngsi_ld_lexicon::{identifier::Identifier, ngsi_ld_date_time::NgsiLdDateTimeError, ngsi_ld_time::NgsiLdTimeError};
use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
use andromeda_postgresql_collation::provisioned_collations::CollationProvisioningError;
use andromeda_postgresql_encoding::ewkb::EwkbError;
use andromeda_postgresql_name_dictionary::name_lookup::NameLookupError;
use andromeda_postgresql_pool::persistence_classification::classify;
use andromeda_postgresql_query::error::RenderError;
use andromeda_postgresql_temporal_query::aggregation_group::MixedAggregationGroup;
use andromeda_temporal::{
    aggregation_conflict::AggregationConflict,
    error::TemporalError,
    temporal_not_found_reason::TemporalNotFoundReason,
    temporal_validation_reason::TemporalValidationReason,
};
use sea_query::error::Error as SeaQueryError;

/// Errors produced by a temporal store implementation.
///
/// Internal to the PostgreSQL adapter boundary: the direct port implementation converts these into [`TemporalError`] without exposing store detail to policy or HTTP clients.
#[derive(Debug, thiserror::Error)]
pub enum TemporalStoreError {
    /// The underlying database query failed.
    #[error("temporal store: {0}")]
    Database(#[from] sqlx::Error),
    /// A history row no longer reads back as the observation it recorded.
    #[error("temporal store history row: {0}")]
    StoredHistory(#[from] StoredHistoryError),
    /// A catalogue row no longer reads back as its domain value.
    #[error("temporal store catalogue row: {0}")]
    StoredCatalog(#[from] StoredCatalogError),
    /// An aggregation row does not read back as the aggregate it computed.
    #[error("temporal store aggregation row: {0}")]
    StoredAggregation(#[from] StoredAggregationError),
    /// A query could not be rendered from the compiled temporal query.
    #[error("temporal store render: {0}")]
    Render(#[source] RenderError),
    /// The requested entity or attribute instance was not found.
    #[error("temporal store not found: {0}")]
    NotFound(TemporalNotFoundReason),
    /// A geometry value could not be encoded for persistence.
    #[error("temporal store encoding: {0}")]
    Encoding(#[from] EwkbError),
    /// An ICU collation a clause-4.23 ordering named could not be created.
    #[error("temporal store collation: {0}")]
    Collation(#[from] CollationProvisioningError),
    /// A name dictionary resolution failed.
    #[error("temporal store name lookup: {0}")]
    NameLookup(#[from] NameLookupError),
    /// A contract between the store's own statements was broken.
    #[error("temporal store invariant: {0}")]
    Invariant(#[from] TemporalStoreInvariant),
    /// The submitted content cannot be stored as written.
    #[error("temporal store refused the submission: {0}")]
    Refused(#[from] SubmissionRefusal),
    /// A write ordinal is outside the range the history column holds, which
    /// means the write carried more entries than the broker can address.
    #[error("temporal store write ordinal: {0}")]
    WriteOrdinal(#[from] WriteOrdinalOutOfRange),
    /// A statement could not be assembled.
    #[error("temporal store statement could not be built: {0}")]
    StatementBuild(#[from] SeaQueryError),
    /// The configured retention horizon reaches before the representable range.
    #[error("temporal store retention: {0}")]
    RetentionHorizon(#[from] TemporalRetentionHorizonInvalid),
    /// A stored mean `Time` does not normalise to a clause 4.6.3 `Time`.
    #[error("temporal store aggregation time: {0}")]
    StoredTime(#[source] NgsiLdTimeError),
    /// An entity scope reached the attribute-history conversion; clause 4.5.4
    /// gives a Scope no attribute stream.
    #[error("temporal store: an entity scope has no attribute history row")]
    ScopeHasNoAttributeRow,
    /// A structured value or geometry members did not write as the JSON its
    /// column stores.
    #[error("temporal store: a stored JSON document does not encode: {0}")]
    StoredJson(#[source] serde_json::Error),
    /// A `DateTime` attribute value could not be accepted at microsecond resolution.
    #[error("temporal store timestamp precision: {0}")]
    TimestampPrecision(#[from] NgsiLdDateTimeError),
    /// An observation's metadata has no stored members.
    #[error("temporal store history members: {0}")]
    HistoryMembers(#[from] HistoryMembersError),
    /// A stored unit identity names no catalog code.
    #[error("temporal store unit code: {0}")]
    UnitCode(#[from] StoredUnitCodeError),
    /// The requested aggregate would have spanned inputs that do not share a meaning.
    #[error("temporal store aggregation: {0}")]
    MixedAggregationGroup(#[from] MixedAggregationGroup),
    /// A client-supplied `instanceId` claimed more than one attribute instance
    /// of the same attribute at once.
    #[error("temporal store instance claim: {0}")]
    DuplicateInstanceClaim(#[from] DuplicateInstanceClaimError),
    /// An append intent carries a write record other than the one its commit
    /// persists, so its history rows would be stamped with an instant the
    /// write record does not hold.
    #[error("temporal store write record: an intent of write {write_id} carries a different write record than the one persisted for its commit")]
    WriteRecordMismatch {
        /// The write the mismatched intent names.
        write_id: WriteId,
    },
    /// Recording one entity's intent failed.
    ///
    /// A chunk records the intents of many entities together, and a refusal
    /// raised while building one intent's rows belongs to that entity's write
    /// alone (clauses 5.6.7 to 5.6.10 report every entity of a batch on its
    /// own). The entity travels beside the failure so the commit can set that
    /// write aside; what the failure means is `source`'s alone.
    #[error("temporal store: recording the history of entity {entity_id} failed: {source}")]
    EntityIntent {
        /// The entity whose intent failed.
        entity_id: Identifier,
        /// What failed.
        #[source]
        source: Box<TemporalStoreError>,
    },
    /// A temporal hypertable already carries the columnstore under segment or
    /// order columns other than the ones the broker enables it with.
    #[error("temporal store columnstore: {hypertable} is {observed}, not the settings the broker enables")]
    ColumnstoreSettingsDrift {
        /// The hypertable whose settings differ.
        hypertable: HypertableName,
        /// The settings it carries.
        observed: Box<StoredColumnstore>,
    },
}

impl TemporalStoreError {
    /// The backend-neutral class of a failure that is the store's own rather
    /// than the request's, or `None` for a refusal that names the request.
    #[must_use]
    pub fn failure_class(&self) -> Option<PersistenceFailure> {
        match self {
            TemporalStoreError::EntityIntent { source, .. } => source.failure_class(),
            TemporalStoreError::Database(error) => Some(classify(error)),
            TemporalStoreError::NameLookup(error) => Some(error.failure_class()),
            TemporalStoreError::StoredHistory(_)
            | TemporalStoreError::StoredCatalog(_)
            | TemporalStoreError::StoredAggregation(_)
            | TemporalStoreError::StoredTime(_)
            | TemporalStoreError::UnitCode(StoredUnitCodeError::Unknown(_)) => Some(PersistenceFailure::Undecodable),
            TemporalStoreError::Render(_) | TemporalStoreError::StatementBuild(_) => Some(PersistenceFailure::StatementUnbuildable),
            TemporalStoreError::Encoding(_)
            | TemporalStoreError::WriteOrdinal(_)
            | TemporalStoreError::StoredJson(_)
            | TemporalStoreError::HistoryMembers(HistoryMembersError::Seals(_)) => Some(PersistenceFailure::Unencodable),
            TemporalStoreError::Collation(CollationProvisioningError::Statement { .. }) => Some(PersistenceFailure::StatementRejected),
            TemporalStoreError::Invariant(TemporalStoreInvariant::NotAHypertable(_) | TemporalStoreInvariant::NoTimeDimension(_)) => {
                Some(PersistenceFailure::SchemaMismatch)
            }
            TemporalStoreError::Invariant(
                TemporalStoreInvariant::AttributeNameUnresolved(_)
                | TemporalStoreInvariant::EntityTypeUnresolved(_)
                | TemporalStoreInvariant::CountStatementMissing
                | TemporalStoreInvariant::CountOutOfRange(_),
            )
            | TemporalStoreError::WriteRecordMismatch { .. }
            | TemporalStoreError::RetentionHorizon(_)
            | TemporalStoreError::ScopeHasNoAttributeRow
            | TemporalStoreError::HistoryMembers(HistoryMembersError::Flatten(_) | HistoryMembersError::UnresolvedType(_))
            | TemporalStoreError::UnitCode(StoredUnitCodeError::Catalog(_)) => Some(PersistenceFailure::ContractBroken),
            TemporalStoreError::ColumnstoreSettingsDrift { .. } => Some(PersistenceFailure::SchemaMismatch),
            TemporalStoreError::NotFound(_)
            | TemporalStoreError::Refused(_)
            | TemporalStoreError::TimestampPrecision(_)
            | TemporalStoreError::MixedAggregationGroup(_)
            | TemporalStoreError::DuplicateInstanceClaim(_)
            | TemporalStoreError::Collation(CollationProvisioningError::NotAnIcuCollation { .. }) => None,
        }
    }
}

impl From<TemporalStoreError> for TemporalError {
    #[track_caller]
    fn from(error: TemporalStoreError) -> TemporalError {
        if let Some(failure) = error.failure_class() {
            return TemporalError::from(failure.record(&error));
        }
        match error {
            TemporalStoreError::EntityIntent { source, .. } => TemporalError::from(*source),
            TemporalStoreError::NotFound(reason) => TemporalError::NotFound(reason),
            TemporalStoreError::Refused(refusal) => TemporalError::from(refusal),
            // A timestamp at a resolution the store cannot hold is client input,
            // and two instances claiming the same instanceId is a conflict inside
            // the submitted write (clause 4.5.7).
            TemporalStoreError::TimestampPrecision(source) => TemporalError::from(SubmissionRefusal::TimestampPrecision(source)),
            TemporalStoreError::DuplicateInstanceClaim(_) => TemporalError::from(SubmissionRefusal::DuplicateInstanceClaim),
            // Which units, families and value shapes the selected instances
            // carry is a property of the request, so an aggregate spanning
            // several of them is refused as client input.
            TemporalStoreError::MixedAggregationGroup(mixed) => TemporalError::Validation(mixed_aggregation(mixed)),
            // Clause 5.7.4.4: "if a preferred collation setting is present and
            // it does not conform to a valid ICU collation (see IETF RFC 6067)
            // then an error of type BadRequestData shall be raised". Whether
            // ICU holds a collation for a locale is a question only the
            // database can answer, so its refusal of the client's own
            // `collation` reaches policy here rather than as a broker fault.
            TemporalStoreError::Collation(CollationProvisioningError::NotAnIcuCollation { collation, .. }) => {
                TemporalError::Validation(TemporalValidationReason::Collation { collation })
            }
            fault @ (TemporalStoreError::Database(_)
            | TemporalStoreError::StoredHistory(_)
            | TemporalStoreError::StoredCatalog(_)
            | TemporalStoreError::StoredAggregation(_)
            | TemporalStoreError::Render(_)
            | TemporalStoreError::Encoding(_)
            | TemporalStoreError::Collation(CollationProvisioningError::Statement { .. })
            | TemporalStoreError::NameLookup(_)
            | TemporalStoreError::Invariant(_)
            | TemporalStoreError::UnitCode(_)
            | TemporalStoreError::WriteRecordMismatch { .. }
            | TemporalStoreError::ColumnstoreSettingsDrift { .. }
            | TemporalStoreError::WriteOrdinal(_)
            | TemporalStoreError::StatementBuild(_)
            | TemporalStoreError::RetentionHorizon(_)
            | TemporalStoreError::StoredTime(_)
            | TemporalStoreError::StoredJson(_)
            | TemporalStoreError::HistoryMembers(_)
            | TemporalStoreError::ScopeHasNoAttributeRow) => TemporalError::from(PersistenceFailure::ContractBroken.record(&fault)),
        }
    }
}

/// The validation reason an aggregation spanning inputs of different meaning
/// is refused with.
fn mixed_aggregation(mixed: MixedAggregationGroup) -> TemporalValidationReason {
    let (attribute, conflict) = match mixed {
        MixedAggregationGroup::ConflictingUnits { attribute_name, .. } => (attribute_name, AggregationConflict::Units),
        MixedAggregationGroup::UnitAndUnitless { attribute_name, .. } => (attribute_name, AggregationConflict::UnitAndUnitless),
        MixedAggregationGroup::ConflictingFamilies { attribute_name, .. } => (attribute_name, AggregationConflict::Families),
        MixedAggregationGroup::ConflictingValueShapes { attribute_name, .. } => (attribute_name, AggregationConflict::ValueShapes),
    };
    TemporalValidationReason::MixedAggregation { attribute, conflict }
}

#[cfg(test)]
mod tests {
    use crate::error::TemporalStoreError;
    use andromeda_context_commit::submission_refusal::SubmissionRefusal;
    use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
    use andromeda_postgresql_name_dictionary::name_lookup::NameLookupError;
    use andromeda_temporal::{error::TemporalError, temporal_validation_reason::TemporalValidationReason};

    #[test]
    fn a_pool_timeout_becomes_store_saturated() {
        let saturated = TemporalError::from(TemporalStoreError::Database(sqlx::Error::PoolTimedOut));
        assert!(matches!(saturated, TemporalError::StoreSaturated));
    }

    #[test]
    fn a_saturated_name_dictionary_becomes_store_saturated() {
        let saturated = TemporalError::from(TemporalStoreError::from(NameLookupError::from(sqlx::Error::PoolTimedOut)));
        assert!(matches!(saturated, TemporalError::StoreSaturated));
    }

    #[test]
    fn a_failed_name_dictionary_statement_stays_a_persistence_fault() {
        let persistence = TemporalError::from(TemporalStoreError::from(NameLookupError::from(sqlx::Error::RowNotFound)));
        assert!(matches!(persistence, TemporalError::Persistence(PersistenceFailure::RecordMissing)));
    }

    #[test]
    fn another_database_failure_stays_a_persistence_fault() {
        let persistence = TemporalError::from(TemporalStoreError::Database(sqlx::Error::RowNotFound));
        assert!(matches!(persistence, TemporalError::Persistence(PersistenceFailure::RecordMissing)));
    }

    #[test]
    fn a_closed_pool_is_the_store_being_unavailable() {
        let unavailable = TemporalError::from(TemporalStoreError::Database(sqlx::Error::PoolClosed));
        assert!(matches!(unavailable, TemporalError::StoreUnavailable));
    }

    #[test]
    fn a_refused_submission_is_a_temporal_validation_failure() {
        let refused = TemporalError::from(TemporalStoreError::Refused(SubmissionRefusal::RelationshipMissingObject));
        assert!(matches!(
            refused,
            TemporalError::Validation(TemporalValidationReason::Refused(SubmissionRefusal::RelationshipMissingObject))
        ));
    }

    /// The entity an intent failure names changes nothing about what the
    /// failure means: a refusal stays a refusal, a fault stays a fault.
    #[test]
    fn an_entity_intent_failure_is_classified_as_its_source() {
        let entity_intent = |source: TemporalStoreError| TemporalStoreError::EntityIntent {
            entity_id: "urn:ngsi-ld:Vehicle:1".parse().expect("a valid entity URI"),
            source: Box::new(source),
        };
        let refused = TemporalError::from(entity_intent(TemporalStoreError::Refused(SubmissionRefusal::RelationshipMissingObject)));
        assert!(matches!(
            refused,
            TemporalError::Validation(TemporalValidationReason::Refused(SubmissionRefusal::RelationshipMissingObject))
        ));
        let fault = entity_intent(TemporalStoreError::ScopeHasNoAttributeRow);
        pretty_assertions::assert_eq!(fault.failure_class(), Some(PersistenceFailure::ContractBroken));
        assert!(matches!(
            TemporalError::from(fault),
            TemporalError::Persistence(PersistenceFailure::ContractBroken)
        ));
    }

    #[test]
    fn a_store_fault_is_classified_rather_than_relayed() {
        let decode = TemporalError::from(TemporalStoreError::ScopeHasNoAttributeRow);
        assert!(matches!(decode, TemporalError::Persistence(PersistenceFailure::ContractBroken)));
    }
}
