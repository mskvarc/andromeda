//! Clause 5.6.15 addresses one instance of one temporal attribute by its `instanceId`, so each operation here runs a statement per identifier column.

use crate::{
    attribute_history_value::AttributeHistoryValue,
    error::TemporalStoreError,
    history_members::{CommittedMetadata, HistoryMembers},
    instance_statements::{InstanceColumn, delete_instance_sql, release_verbatim_instance_claim_sql, select_instance_sql, update_instance_sql},
    repo::TemporalRepository,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::{
    attribute_name::AttributeName,
    attribute_type::AttributeType,
    identifier::Identifier,
    instance_identity::InstanceIdentity,
    uri::Uri,
};
use andromeda_postgresql_attribute_instance::{
    name_keys::MentionedNames,
    name_resolution::resolve_mentioned,
    stored_json::StoredJson,
    sub_attribute::SubAttributeRecord,
};
use andromeda_postgresql_codes::attribute_kind_code::stored_attribute_kind;
use andromeda_postgresql_encoding::{ewkb::EwkbError, ewkb_binding::geometry_bytes};
use andromeda_postgresql_query::entity_type_name_key::EntityTypeNameKey;
use andromeda_temporal::{observed_instance::ObservedInstance, temporal_not_found_reason::TemporalNotFoundReason};
use andromeda_unit_catalog::unit_code_id::UnitCodeId;
use chrono::{DateTime, Utc};
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
        let state = UpdateInstanceState::of_replacement(attribute_name, replacement)?;
        self.update_instance(target, entity_id, attribute_name, instance, state).await
    }

    /// Physically remove one named attribute instance from history.
    ///
    /// Clause 5.6.15 deletes the addressed instance itself rather than
    /// recording that it ended, so the row is removed and no tombstone is
    /// written: a tombstone would keep answering temporal queries for an
    /// instance the client asked to have removed.
    ///
    /// Deliberate retention asymmetry: unlike whole-entity temporal deletion, this path does not read the removed row's `write_id` and does not release its write record. Other rows of that write may still exist, and a single deleted row cannot show whether the record became unreferenced. Retaining it costs one record's storage until the retention sweep, which knows the full set of referents, reaps it; releasing early would corrupt replay identity for sibling rows.
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

    /// Modify one named attribute instance in place, minting the names its
    /// members hold in the same transaction.
    async fn update_instance(
        &self,
        target: &ContextTarget,
        entity_id: &Identifier,
        attribute_name: &AttributeName,
        instance: &InstanceIdentity,
        state: UpdateInstanceState,
    ) -> Result<(), TemporalStoreError> {
        let mut tx = self.pool.begin().await?;
        let mut mentioned = MentionedNames::default();
        state.metadata.mention(&mut mentioned);
        let keys = resolve_mentioned(&mut tx, mentioned, self.entity_type_names.as_ref(), self.attribute_names.as_ref()).await?;
        let HistoryMembers {
            value_type_key,
            object_type_keys,
            unit_code_id,
            expires_at,
            proof_seals,
            sub_attributes,
        } = state.metadata.keyed(&keys)?;
        let elements = sub_attributes
            .into_iter()
            .map(SubAttributeRecord::try_from)
            .collect::<Result<Vec<_>, EwkbError>>()?;
        let object_type_keys: Vec<i32> = object_type_keys.into_iter().map(EntityTypeNameKey::get).collect();
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
            .bind(geometry_bytes(state.value.geometry())?)
            .bind(state.value.object().map(Json))
            .bind(state.value.date_time())
            .bind(state.value.uri().map(Uri::as_str))
            .bind(state.observed_at)
            .bind(value_type_key.map(EntityTypeNameKey::get))
            .bind(unit_code_id.map(UnitCodeId::get))
            .bind(stored_attribute_kind(state.attribute_type).code())
            .bind(state.value.geometry_members().map(Json))
            .bind((!object_type_keys.is_empty()).then_some(object_type_keys))
            .bind(proof_seals.map(StoredJson::into_json))
            .bind(expires_at)
            .bind((!elements.is_empty()).then_some(elements))
            .execute(&mut *tx)
            .await?
            .rows_affected();
        if updated == 0 {
            return Err(TemporalStoreError::NotFound(instance_not_found(instance)));
        }
        tx.commit().await?;
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
    /// The replacement's metadata in the committed model, its unit lifted to
    /// its catalog identity.
    metadata: CommittedMetadata,
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
    /// The update one replacement observation of `attribute_name` makes.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalStoreError`] when the value or the metadata does not
    /// encode, or the `unitCode` names no unit a row can hold.
    fn of_replacement(attribute_name: &AttributeName, replacement: &ObservedInstance) -> Result<UpdateInstanceState, TemporalStoreError> {
        Ok(UpdateInstanceState {
            value: AttributeHistoryValue::of_observation(&replacement.value)?,
            metadata: CommittedMetadata::of(attribute_name, replacement.metadata.as_ref())?,
            observed_at: replacement.observed_at,
            attribute_type: replacement.attribute_type,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::{history_members::HistoryMembers, instance_mutation::UpdateInstanceState};
    use andromeda_ngsi_ld_entity::document::instance_document::InstanceDocument;
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use andromeda_postgresql_attribute_instance::name_keys::NameKeys;
    use andromeda_postgresql_codes::value_kind::ValueKind;
    use andromeda_postgresql_query::{attribute_name_key::AttributeNameKey, dictionary::resolved_names::ResolvedNames};
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
        UpdateInstanceState::of_replacement(&speed, &replacement).expect("the replacement encodes")
    }

    /// The clause 5.6.15 replacement is read by the same decoder an append is, so an array-valued Property replaced in place keeps every element, as the structured column stores it.
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

    /// Clause 5.6.15 replaces the whole instance: its sub-attribute is bound
    /// as a typed element and its unit as its catalog identity, and no member
    /// travels as JSON.
    #[test]
    fn a_replacement_instance_binds_its_typed_metadata() {
        let update = update_of(&json!({
            "@type": ["https://uri.etsi.org/ngsi-ld/ListProperty"],
            "https://uri.etsi.org/ngsi-ld/hasValueList": [{"@list": [{"@value": 1}]}],
            "https://uri.etsi.org/ngsi-ld/createdAt": [{"@value": "2024-01-01T00:00:00.000000Z", "@type": "https://uri.etsi.org/ngsi-ld/DateTime"}],
            "https://uri.etsi.org/ngsi-ld/unitCode": [{"@value": "MTR"}],
            "https://example.org/accuracy": [{"@type": ["https://uri.etsi.org/ngsi-ld/Property"], "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 0.5}]}]
        }));
        let accuracy: AttributeName = "https://example.org/accuracy".parse().expect("a valid attribute name");
        let keys = NameKeys::new(ResolvedNames::from_pairs([(accuracy, AttributeNameKey::from(7))]), ResolvedNames::default());
        let members: HistoryMembers = update.metadata.keyed(&keys).expect("the metadata keys");
        assert!(members.unit_code_id.is_some());
        let keyed: Vec<(i32, AttributeNameKey)> = members
            .sub_attributes
            .iter()
            .map(|element| (element.parent, element.members.attribute_name_key))
            .collect();
        assert_eq!(keyed, vec![(0, AttributeNameKey::from(7))]);
        assert!(members.proof_seals.is_none() && members.value_type_key.is_none() && members.object_type_keys.is_empty());
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
