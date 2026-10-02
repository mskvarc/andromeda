//! Mutation and retrieval of individual temporal attribute instances.
//!
//! Clause 5.6.15 addresses one instance of one temporal attribute by its
//! `instanceId`; the statements doing so, one per identifier column, live in
//! [`crate::instance_statements`].

use crate::{
    attribute_history_value::AttributeHistoryValue,
    error::TemporalStoreError,
    instance_statements::{InstanceColumn, delete_instance_sql, release_verbatim_instance_claim_sql, select_instance_sql, update_instance_sql},
    repo::TemporalRepository,
    unit_code_metadata::take_unit_code,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_geometry::geometry::Geometry;
use andromeda_ngsi_ld_lexicon::{
    attribute_name::AttributeName,
    attribute_type::AttributeType,
    identifier::Identifier,
    instance_identity::InstanceIdentity,
    uri::Uri,
};
use andromeda_ngsi_ld_model::temporal::observation_metadata::ObservationMetadata;
use andromeda_postgresql_codes::attribute_kind_code::stored_attribute_kind;
use andromeda_postgresql_encoding::ewkb_binding::geometry_bytes;
use andromeda_temporal::{observed_instance::ObservedInstance, temporal_not_found_reason::TemporalNotFoundReason};
use andromeda_unit_catalog::unit_code_id::UnitCodeId;
use chrono::{DateTime, Utc};
use serde_json::value::RawValue;
use sqlx::{AssertSqlSafe, postgres::PgRow, types::Json};

impl TemporalRepository {
    /// Replace one attribute instance with the observation `replacement`
    /// records (clause 5.6.14).
    pub(crate) async fn update_attribute_instance(
        &self,
        target: &ContextTarget,
        entity_id: &Identifier,
        attribute_name: &AttributeName,
        instance: &InstanceIdentity,
        replacement: &ObservedInstance,
    ) -> Result<(), TemporalStoreError> {
        let state = UpdateInstanceState::of_replacement(replacement)?;
        self.update_instance(target, entity_id, attribute_name, instance, &state).await
    }

    /// Physically remove one named attribute instance from history.
    ///
    /// Clause 5.6.15 deletes the addressed instance itself rather than
    /// recording that it ended, so the row is removed and no tombstone is
    /// written: a tombstone would keep answering temporal queries for an
    /// instance the client asked to have removed.
    ///
    /// Deliberate retention asymmetry: unlike whole-entity temporal deletion,
    /// this single-instance path does not read the removed row's `write_id`
    /// and does not release the write record it belongs to. Every other row
    /// of that write (other instances, other attributes, current state,
    /// notifications, side effects) may still exist, so this statement cannot
    /// know from a single deleted row whether the write record it belonged to
    /// has become unreferenced. Leaving the write record retained is the safe
    /// direction: retention's own periodic sweep is what actually knows the
    /// full set of referents and reaps a write record once none of them
    /// remain, so retaining here costs one write record's worth of storage
    /// until that sweep runs, whereas guessing wrong and releasing early would
    /// corrupt replay identity for every sibling row still pointing at it.
    pub(crate) async fn delete_instance(
        &self,
        target: &ContextTarget,
        instance: &InstanceIdentity,
        entity_id: &Identifier,
        attribute_name: &AttributeName,
    ) -> Result<(), TemporalStoreError> {
        let mut tx = self.pool.begin().await?;
        let generated_sql = delete_instance_sql(InstanceColumn::Generated, target);
        let verbatim_sql = delete_instance_sql(InstanceColumn::Verbatim, target);
        let statement = match instance {
            InstanceIdentity::Generated(uuid) => sqlx::query(AssertSqlSafe(generated_sql)).bind(uuid),
            InstanceIdentity::Verbatim(identifier) => sqlx::query(AssertSqlSafe(verbatim_sql)).bind(identifier.as_str()),
        };
        let deleted = statement
            .bind(entity_id.as_str())
            .bind(attribute_name.as_str())
            .execute(&mut *tx)
            .await?
            .rows_affected();
        if deleted == 0 {
            return Err(TemporalStoreError::NotFound(instance_not_found(instance)));
        }
        // A broker-minted identifier never claims a row in the first place
        // (`duplicate_instance_claim`), so only a verbatim one has a claim to
        // release. Releasing it here is what lets a client reuse the
        // `instanceId` of an instance it has just deleted.
        if let InstanceIdentity::Verbatim(identifier) = instance {
            let release_sql = release_verbatim_instance_claim_sql(target);
            sqlx::query(AssertSqlSafe(release_sql))
                .bind(identifier.as_str())
                .bind(entity_id.as_str())
                .bind(attribute_name.as_str())
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// Modify one named attribute instance in place.
    async fn update_instance(
        &self,
        target: &ContextTarget,
        entity_id: &Identifier,
        attribute_name: &AttributeName,
        instance: &InstanceIdentity,
        state: &UpdateInstanceState,
    ) -> Result<(), TemporalStoreError> {
        let generated_sql = update_instance_sql(InstanceColumn::Generated, target);
        let verbatim_sql = update_instance_sql(InstanceColumn::Verbatim, target);
        let statement = match instance {
            InstanceIdentity::Generated(uuid) => sqlx::query(AssertSqlSafe(generated_sql)).bind(uuid),
            InstanceIdentity::Verbatim(identifier) => sqlx::query(AssertSqlSafe(verbatim_sql)).bind(identifier.as_str()),
        };
        let updated = statement
            .bind(entity_id.as_str())
            .bind(attribute_name.as_str())
            .bind(state.value.kind().code())
            .bind(state.value.number())
            .bind(state.value.string())
            .bind(state.value.boolean())
            .bind(geometry_bytes(state.value.geometry().map(Geometry::form))?)
            .bind(state.value.object().map(Json))
            .bind(state.value.date_time())
            .bind(state.value.uri().map(Uri::as_str))
            .bind(state.observed_at)
            .bind(state.metadata.as_deref().map(Json))
            .bind(state.unit_code_id.map(UnitCodeId::get))
            .bind(stored_attribute_kind(state.attribute_type).code())
            .execute(&self.pool)
            .await?
            .rows_affected();
        if updated == 0 {
            return Err(TemporalStoreError::NotFound(instance_not_found(instance)));
        }
        Ok(())
    }

    /// Fetch a single attribute instance for policy validation.
    pub(crate) async fn get_instance(
        &self,
        target: &ContextTarget,
        instance: &InstanceIdentity,
        entity_id: &Identifier,
        attribute_name: &AttributeName,
    ) -> Result<Option<PgRow>, TemporalStoreError> {
        let generated_sql = select_instance_sql(InstanceColumn::Generated, target);
        let verbatim_sql = select_instance_sql(InstanceColumn::Verbatim, target);
        let statement = match instance {
            InstanceIdentity::Generated(uuid) => sqlx::query(AssertSqlSafe(generated_sql)).bind(uuid),
            InstanceIdentity::Verbatim(identifier) => sqlx::query(AssertSqlSafe(verbatim_sql)).bind(identifier.as_str()),
        };
        Ok(statement
            .bind(entity_id.as_str())
            .bind(attribute_name.as_str())
            .fetch_optional(&self.pool)
            .await?)
    }
}

/// Report an instance the addressed attribute does not hold.
fn instance_not_found(instance: &InstanceIdentity) -> TemporalNotFoundReason {
    TemporalNotFoundReason::AttributeInstance {
        instance: instance.clone(), // clone: the reason owns the instance it names
    }
}

/// Owned update values kept alive while PostgreSQL binds borrow them.
struct UpdateInstanceState {
    /// The replacement's value, in the shape and column the row stores it in.
    ///
    /// A replacement always carries a value, so this is never a tombstone:
    /// that shape is reserved for a row recording a deletion (clauses 4.5.7
    /// and 4.5.8), which clause 5.6.15 cannot produce.
    value: AttributeHistoryValue,
    /// The stored JSON of the replacement's metadata, without the `unitCode`
    /// member.
    metadata: Option<Box<RawValue>>,
    /// Catalog identity of the instance's `unitCode`, when it declares one.
    unit_code_id: Option<UnitCodeId>,
    /// The observation instant the modified instance declares, if any.
    observed_at: Option<DateTime<Utc>>,
    /// The attribute family of the replacement instance.
    ///
    /// Clause 5.6.14 defines instance PATCH as replacement by the supplied
    /// complete fragment, so a replacement that changes family must move the
    /// stored `attribute_kind` along with the value columns: retaining it
    /// here is what lets the update statement keep the two in agreement
    /// rather than rendering the old family over the new value shape.
    attribute_type: AttributeType,
}

impl UpdateInstanceState {
    /// The update one replacement observation makes.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalStoreError`] when the value or the metadata does not
    /// encode, or the `unitCode` names no unit a row can hold.
    fn of_replacement(replacement: &ObservedInstance) -> Result<UpdateInstanceState, TemporalStoreError> {
        // clone: the update owns its metadata, the replacement keeps its own.
        let (metadata, unit_code_id) = take_unit_code(replacement.metadata.clone())?;
        Ok(UpdateInstanceState {
            value: AttributeHistoryValue::of_observation(&replacement.value)?,
            metadata: metadata
                .as_ref()
                .map(ObservationMetadata::to_stored_json)
                .transpose()
                .map_err(TemporalStoreError::StoredJson)?,
            unit_code_id,
            observed_at: replacement.observed_at,
            attribute_type: replacement.attribute_type,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::instance_mutation::UpdateInstanceState;
    use andromeda_ngsi_ld_entity::document::instance_document::InstanceDocument;
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use andromeda_postgresql_codes::value_kind::ValueKind;
    use andromeda_temporal::observed_instance::ObservedInstance;
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};

    /// The update the expanded replacement `instance` of `speed` makes.
    fn update_of(instance: &Value) -> UpdateInstanceState {
        let speed: AttributeName = "https://example.org/speed".parse().expect("a valid attribute name");
        let document = InstanceDocument::from_slice(&speed, &serde_json::to_vec(instance).expect("JSON")).expect("an expanded instance");
        let replacement = ObservedInstance::read(document)
            .expect("a readable replacement")
            .expect("a replacement records a value");
        UpdateInstanceState::of_replacement(&replacement).expect("the replacement encodes")
    }

    /// The clause 5.6.15 replacement is read by the same decoder an append is,
    /// so an array-valued Property replaced in place keeps every element, as
    /// the structured column stores it, where the store's own reader kept the
    /// first element and dropped the rest.
    #[test]
    fn a_replaced_array_valued_property_stores_every_element() {
        let update = update_of(&json!({
            "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
            "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 1}, {"@value": 2}, {"@value": 3}]
        }));
        assert_eq!(update.value.kind(), ValueKind::Object);
        let stored: Value = serde_json::from_str(update.value.object().expect("the structured column").get()).expect("parses");
        assert_eq!(stored, json!([1, 2, 3]));
    }

    /// The replacement's metadata holds its sub-attributes and companions
    /// alone: the value, the plural and list members, and the clause 4.8 times
    /// the store's own reader used to keep there, where they overrode the
    /// rendered value, are not metadata; the unit moves to its column.
    #[test]
    fn a_replacement_stores_only_its_metadata_in_the_metadata_column() {
        let accuracy = json!([{"@type": ["https://uri.etsi.org/ngsi-ld/Property"], "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 0.5}]}]);
        let update = update_of(&json!({
            "@type": ["https://uri.etsi.org/ngsi-ld/ListProperty"],
            "https://uri.etsi.org/ngsi-ld/hasValueList": [{"@list": [{"@value": 1}]}],
            "https://uri.etsi.org/ngsi-ld/createdAt": [{"@value": "2024-01-01T00:00:00.000000Z", "@type": "https://uri.etsi.org/ngsi-ld/DateTime"}],
            "https://uri.etsi.org/ngsi-ld/modifiedAt": [{"@value": "2024-01-01T00:00:00.000000Z", "@type": "https://uri.etsi.org/ngsi-ld/DateTime"}],
            "https://uri.etsi.org/ngsi-ld/unitCode": [{"@value": "MTR"}],
            "https://example.org/accuracy": accuracy
        }));
        let metadata: Value = serde_json::from_str(update.metadata.as_deref().expect("the sub-attribute is metadata").get()).expect("parses");
        assert_eq!(metadata, json!({"https://example.org/accuracy": accuracy}));
        assert!(update.unit_code_id.is_some());
        let value: Value = serde_json::from_str(update.value.object().expect("the structured column").get()).expect("parses");
        assert_eq!(value, json!([{"@value": 1}]));
    }

    /// A replacement in a typed column keeps that column's kind.
    #[test]
    fn a_scalar_replacement_lands_in_its_typed_column() {
        for (value, kind) in [
            (json!({"@value": 12.5}), ValueKind::Number),
            (json!({"@value": "text"}), ValueKind::String),
            (json!({"@value": true}), ValueKind::Boolean),
            (json!({"@value": "2026-07-25T10:00:00Z"}), ValueKind::DateTime),
        ] {
            let update = update_of(&json!({"@type": ["https://uri.etsi.org/ngsi-ld/Property"], "https://uri.etsi.org/ngsi-ld/hasValue": [value]}));
            assert_eq!(update.value.kind(), kind, "{value}");
        }
    }
}
