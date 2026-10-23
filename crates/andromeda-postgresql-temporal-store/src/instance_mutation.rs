//! Clause 5.6.15 addresses one instance of one temporal attribute by its `instanceId`, so each operation here addresses the identifier column the identity is stored in.

use crate::{
    attribute_history_value::AttributeHistoryValue,
    error::TemporalStoreError,
    history_members::{HistoryMembers, mention},
    instance_statements::{
        InstanceReplacement,
        attribute_stream_keys_statement,
        delete_instance_statement,
        grow_stream_extent_statement,
        grow_stream_shapes_statement,
        release_verbatim_instance_claim_statement,
        select_instance_statement,
        update_instance_statement,
    },
    repo::TemporalRepository,
    stream_shape_pruning::{lock_stream_evolutions, prune_stream_shapes},
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::{
    attribute_name::AttributeName,
    attribute_type::AttributeType,
    entity_type_name::EntityTypeName,
    identifier::Identifier,
    instance_identity::InstanceIdentity,
};
use andromeda_ngsi_ld_model::temporal::{TemporalValue, observation_metadata::ObservationMetadata};
use andromeda_postgresql_attribute_instance::{
    name_keys::MentionedNames,
    name_resolution::resolve_mentioned,
    stored_json::StoredJson,
    sub_attribute::SubAttributeRecord,
};
use andromeda_postgresql_branch::schema_iden;
use andromeda_postgresql_codes::{attribute_kind_code::stored_attribute_kind, stored_unit_code::stored_unit_code};
use andromeda_postgresql_encoding::{ewkb::EwkbError, ewkb_binding::geometry_bytes};
use andromeda_postgresql_query::{attribute_name_key::AttributeNameKey, entity_type_name_key::EntityTypeNameKey};
use andromeda_postgresql_seaquery::{
    external_arguments::{ExternalArguments, ExternallyBound},
    statement::Statement,
};
use andromeda_temporal::{observed_instance::ObservedInstance, temporal_not_found_reason::TemporalNotFoundReason};
use chrono::{DateTime, Utc};
use sqlx::{PgConnection, postgres::PgRow};

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
        let state = UpdateInstanceState::of_replacement(replacement);
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
        // An attribute no write ever recorded holds no instance.
        let Some(attribute) = self.attribute_key(attribute_name).await? else {
            return Err(TemporalStoreError::NotFound(instance_not_found(instance)));
        };
        let mut tx = self.pool.begin().await?;
        let schema = schema_iden(target);
        let stream_keys = attribute_stream_keys(&mut tx, target, entity_id, attribute).await?;
        lock_stream_evolutions(&mut tx, schema.as_ref(), &stream_keys).await?;
        let deleted = delete_instance_statement(instance, target, &stream_keys)
            .to_query()
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
            release_verbatim_instance_claim_statement(target, identifier, entity_id, attribute)
                .to_query()
                .execute(&mut *tx)
                .await?;
        }
        prune_stream_shapes(&mut tx, schema.as_ref(), &stream_keys).await?;
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
        state: UpdateInstanceState<'_>,
    ) -> Result<(), TemporalStoreError> {
        let Some(attribute) = self.attribute_key(attribute_name).await? else {
            return Err(TemporalStoreError::NotFound(instance_not_found(instance)));
        };
        let mut tx = self.pool.begin().await?;
        let mut mentioned = MentionedNames::default();
        if let Some(metadata) = state.metadata {
            mention(metadata, &mut mentioned);
        }
        mentioned.types.extend(state.value.datatype().map(EntityTypeName::from));
        let keys = resolve_mentioned(&mut tx, mentioned, self.entity_type_names.as_ref(), self.attribute_names.as_ref()).await?;
        let value = AttributeHistoryValue::of_observation(state.value, &keys)?;
        let HistoryMembers {
            value_type_key,
            object_type_keys,
            unit_code,
            expires_at,
            proof_seals,
            sub_attributes,
        } = state
            .metadata
            .map(|metadata| HistoryMembers::keyed(metadata, &keys))
            .transpose()?
            .unwrap_or_default();
        let elements = sub_attributes
            .into_iter()
            .map(SubAttributeRecord::try_from)
            .collect::<Result<Vec<_>, EwkbError>>()?;
        let object_type_keys: Vec<i32> = object_type_keys.into_iter().map(EntityTypeNameKey::get).collect();
        let schema = schema_iden(target);
        let stream_keys = attribute_stream_keys(&mut tx, target, entity_id, attribute).await?;
        lock_stream_evolutions(&mut tx, schema.as_ref(), &stream_keys).await?;
        let geometry = geometry_bytes(value.geometry())?;
        let mut external = ExternalArguments::default();
        let sub_attributes = external.push((!elements.is_empty()).then_some(elements));
        let replacement = InstanceReplacement {
            value: &value,
            geometry: geometry.as_deref(),
            observed_at: state.observed_at,
            value_type_key: value_type_key.map(EntityTypeNameKey::get),
            unit_code: unit_code.map(stored_unit_code),
            attribute_kind: stored_attribute_kind(state.attribute_type).code(),
            object_type_keys,
            proof_seals: proof_seals.as_ref().map(StoredJson::get),
            expires_at,
        };
        let update = update_instance_statement(instance, target, &stream_keys, replacement, sub_attributes);
        let modified: Vec<i64> = ExternallyBound::new(update, external).into_query_scalar().fetch_all(&mut *tx).await?;
        if modified.is_empty() {
            return Err(TemporalStoreError::NotFound(instance_not_found(instance)));
        }
        if let Some(geometry) = geometry.as_deref() {
            grow_stream_extent_statement(target, &modified, geometry).to_query().execute(&mut *tx).await?;
        }
        prune_stream_shapes(&mut tx, schema.as_ref(), &modified).await?;
        grow_stream_shapes_statement(instance, target, &modified).to_query().execute(&mut *tx).await?;
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
        let Some(attribute) = self.attribute_key(attribute_name).await? else {
            return Ok(None);
        };
        Ok(select_instance_statement(instance, target, entity_id, attribute)
            .to_query()
            .fetch_optional(&self.pool)
            .await?)
    }
}

/// The stream keys of `entity_id`'s attribute keyed `attribute`, which the
/// instance statements bind as values.
async fn attribute_stream_keys(
    connection: &mut PgConnection,
    target: &ContextTarget,
    entity_id: &Identifier,
    attribute: AttributeNameKey,
) -> Result<Vec<i64>, TemporalStoreError> {
    Ok(attribute_stream_keys_statement(target, entity_id, attribute)
        .to_query_scalar()
        .fetch_all(connection)
        .await?)
}

/// Report an instance the addressed attribute does not hold.
fn instance_not_found(instance: &InstanceIdentity) -> TemporalNotFoundReason {
    TemporalNotFoundReason::AttributeInstance {
        instance: instance.clone(), // clone: the reason owns the instance it names
    }
}

/// What one replacement writes, borrowed from the observation it records.
struct UpdateInstanceState<'a> {
    /// The replacement's value, encoded once its datatype has a key.
    ///
    /// A replacement always carries a value, so this is never a tombstone:
    /// that shape is reserved for a row recording a deletion (clauses 4.5.7
    /// and 4.5.8), which clause 5.6.15 cannot produce.
    value: &'a TemporalValue,
    /// The replacement's metadata.
    metadata: Option<&'a ObservationMetadata>,
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

impl UpdateInstanceState<'_> {
    /// The update one replacement observation makes.
    const fn of_replacement(replacement: &ObservedInstance) -> UpdateInstanceState<'_> {
        UpdateInstanceState {
            value: &replacement.value,
            metadata: replacement.metadata.as_ref(),
            observed_at: replacement.observed_at,
            attribute_type: replacement.attribute_type,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{attribute_history_value::AttributeHistoryValue, history_members::HistoryMembers, instance_mutation::UpdateInstanceState};
    use andromeda_ngsi_ld_entity::document::instance_document::InstanceDocument;
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName};
    use andromeda_postgresql_attribute_instance::name_keys::NameKeys;
    use andromeda_postgresql_codes::value_kind::ValueKind;
    use andromeda_postgresql_query::{
        attribute_name_key::AttributeNameKey,
        dictionary::resolved_names::ResolvedNames,
        entity_type_name_key::EntityTypeNameKey,
    };
    use andromeda_temporal::observed_instance::ObservedInstance;
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};

    /// The expanded replacement `instance` of `speed`, read as an observation.
    fn replacement_of(instance: &Value) -> ObservedInstance {
        let speed: AttributeName = "https://example.org/speed".parse().expect("a valid attribute name");
        let document = InstanceDocument::from_slice(&speed, &serde_json::to_vec(instance).expect("JSON")).expect("an expanded instance");
        ObservedInstance::read(document)
            .expect("a readable replacement")
            .expect("a replacement records a value")
    }

    /// The update `replacement` makes.
    fn update_of(replacement: &ObservedInstance) -> UpdateInstanceState<'_> {
        UpdateInstanceState::of_replacement(replacement)
    }

    /// The row value `update` writes, the three time datatypes keyed.
    fn encoded(update: &UpdateInstanceState<'_>) -> AttributeHistoryValue {
        let datatypes = ["DateTime", "Date", "Time"].into_iter().zip(1..).map(|(form, key)| {
            (
                format!("https://uri.etsi.org/ngsi-ld/{form}").parse::<EntityTypeName>().expect("a datatype"),
                EntityTypeNameKey::from(key),
            )
        });
        AttributeHistoryValue::of_observation(update.value, &NameKeys::new(ResolvedNames::default(), ResolvedNames::from_pairs(datatypes)))
            .expect("the value encodes")
    }

    /// The clause 5.6.15 replacement is read by the same decoder an append is, so an array-valued Property replaced in place keeps every element, as the structured column stores it.
    #[test]
    fn a_replaced_array_valued_property_stores_every_element() {
        let replacement = replacement_of(&json!({
            "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
            "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 1}, {"@value": 2}, {"@value": 3}]
        }));
        let value = encoded(&update_of(&replacement));
        assert_eq!(value.kind(), ValueKind::Object);
        let stored: Value = serde_json::from_str(value.object().expect("the structured column").get()).expect("parses");
        assert_eq!(stored, json!([1, 2, 3]));
    }

    /// Clause 5.6.15 replaces the whole instance: its sub-attribute is bound
    /// as a typed element and its unit as its stored identity, and no member
    /// travels as JSON.
    #[test]
    fn a_replacement_instance_binds_its_typed_metadata() {
        let replacement = replacement_of(&json!({
            "@type": ["https://uri.etsi.org/ngsi-ld/ListProperty"],
            "https://uri.etsi.org/ngsi-ld/hasValueList": [{"@list": [{"@value": 1}]}],
            "https://uri.etsi.org/ngsi-ld/createdAt": [{"@value": "2024-01-01T00:00:00.000000Z", "@type": "https://uri.etsi.org/ngsi-ld/DateTime"}],
            "https://uri.etsi.org/ngsi-ld/unitCode": [{"@value": "MTR"}],
            "https://example.org/accuracy": [{"@type": ["https://uri.etsi.org/ngsi-ld/Property"], "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 0.5}]}]
        }));
        let update = update_of(&replacement);
        let accuracy: AttributeName = "https://example.org/accuracy".parse().expect("a valid attribute name");
        let keys = NameKeys::new(ResolvedNames::from_pairs([(accuracy, AttributeNameKey::from(7))]), ResolvedNames::default());
        let members = HistoryMembers::keyed(update.metadata.expect("the replacement carries metadata"), &keys).expect("the metadata keys");
        assert!(members.unit_code.is_some());
        let keyed: Vec<(i32, AttributeNameKey)> = members
            .sub_attributes
            .iter()
            .map(|element| (element.parent, element.members.attribute_name_key))
            .collect();
        assert_eq!(keyed, vec![(0, AttributeNameKey::from(7))]);
        assert!(members.proof_seals.is_none() && members.value_type_key.is_none() && members.object_type_keys.is_empty());
        let value: Value = serde_json::from_str(encoded(&update).object().expect("the structured column").get()).expect("parses");
        assert_eq!(value, json!([{"@value": 1}]));
    }

    /// A replacement in a typed column keeps that column's kind.
    #[test]
    fn a_scalar_replacement_lands_in_its_typed_column() {
        for (value, kind) in [
            (json!({"@value": 12.5}), ValueKind::Number),
            (json!({"@value": "text"}), ValueKind::String),
            (json!({"@value": true}), ValueKind::Boolean),
            (json!({"@value": "2026-07-25T10:00:00Z"}), ValueKind::String),
            (
                json!({"@value": "2026-07-25T10:00:00Z", "@type": "https://uri.etsi.org/ngsi-ld/DateTime"}),
                ValueKind::DateTime,
            ),
            (json!({"@value": "2026-07-25", "@type": "https://uri.etsi.org/ngsi-ld/Date"}), ValueKind::Date),
            (json!({"@value": "10:00:00Z", "@type": "https://uri.etsi.org/ngsi-ld/Time"}), ValueKind::Time),
        ] {
            let replacement = replacement_of(&json!({"@type": ["https://uri.etsi.org/ngsi-ld/Property"], "https://uri.etsi.org/ngsi-ld/hasValue": [value]}));
            assert_eq!(encoded(&update_of(&replacement)).kind(), kind, "{value}");
        }
    }
}
