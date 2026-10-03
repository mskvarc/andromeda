//! Batch attribute update (clause 5.6.9).
//!
//! A read-modify-write over the whole chunk: the stored payloads are read
//! together, each request fragment is applied in memory, and the results go
//! back as one statement, each holding only while its entity is unchanged
//! since the read (see [`crate::batch::chunk_read_modify_write`]).
//!
//! Reading the stored payloads together is only sound because a chunk never
//! carries two instances of the same entity: the coordinator gives each instance of a
//! repeated identifier a round of its own, so the second instance reads what
//! the first committed. Clause 5.5.11.3 requires exactly that, one separate
//! notification per individual update.

use crate::{
    EntityService,
    batch::{
        BatchResult,
        admitted_batch::AdmittedBatchEntity,
        chunk::UpdatedStates,
        chunk_read_modify_write::{ChunkRewrite, PlannedWrite},
        prepare::PreparedEntity,
    },
    entity_snapshot::EntitySnapshot,
    entity_validation_reason::EntityValidationReason,
    error::EntityError,
    side_effect::SideEffectPlacement,
    write_admission::BatchWrite,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_entity::{
    document::EntityDocument,
    entity_state::EntityState,
    history::written_observation::AddressedInstances,
    operation::selection::OverwritePolicy,
    system_instant::SystemInstant,
};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;

/// Whether a `noOverwrite` append of `fragment` onto `entity` can add nothing:
/// the fragment names at least one member and every member it names is
/// already present. Clause 5.6.9.5 counts an Entity in S only when it was
/// "successfully updated (Attributes appended)", so an append that adds no
/// attribute leaves the Entity out of S (clause 5.6.9.4). A fragment that names
/// a not-yet-present attribute appends it and the Entity is a success even if
/// others are skipped, matching the single Append Attributes operation (clause
/// 5.6.3), which reports the skipped ones in a `notUpdated` list rather than
/// failing the Entity.
fn appends_nothing(entity: &EntityState, fragment: &EntityDocument) -> bool {
    let scope = fragment.scope().map(|_| entity.scope().is_some());
    let expiry = fragment.expires_at().map(|_| entity.expires_at().is_some());
    let attributes = fragment.attributes().keys().map(|name| entity.attributes().contains(name.as_str()));
    let mut named = scope.into_iter().chain(expiry).chain(attributes).peekable();
    named.peek().is_some() && named.all(|present| present)
}

/// Updating each entity of a chunk as an attempt read it (clause 5.6.9.4 runs
/// the behaviour of clause 5.6.3 per entity).
struct UpdateRewrite<'a> {
    service: &'a EntityService,
    target: &'a ContextTarget,
    overwrite: OverwritePolicy,
}

impl ChunkRewrite for UpdateRewrite<'_> {
    type Entry = PreparedEntity;
    type Reads = ();

    fn id(entry: &PreparedEntity) -> &Identifier {
        &entry.id
    }

    fn build(
        &self,
        entry: PreparedEntity,
        snapshot: Option<&EntitySnapshot>,
        _reads: &(),
        now: SystemInstant,
    ) -> Result<PlannedWrite<PreparedEntity>, Box<(Identifier, EntityError)>> {
        let Some(snapshot) = snapshot else {
            let error = EntityError::not_found(&entry.id);
            return Err(Box::new((entry.id, error)));
        };
        let previous = &snapshot.state;
        // clone: reference-count bumps; the update shares every attribute it does not touch with the stored state
        let mut updated = EntityState::clone(previous);

        let mut alongside = None;
        let applied = if self.overwrite == OverwritePolicy::Preserve {
            // Clause 5.6.9.4: the local phase is an Append Attributes (clause
            // 5.6.3) with overwrite disabled. Clause 5.6.9.5 puts an Entity in
            // S when it was "successfully updated (Attributes appended)", so a
            // fragment that appends at least one attribute is a success even if
            // others are skipped for noOverwrite; only a fragment that appends
            // nothing (every named attribute already present) leaves the Entity
            // out of S and is reported as an error, making the batch a 207.
            // Whether it appended anything depends on the state this attempt
            // read, so the error belongs to the attempt.
            if appends_nothing(&updated, &entry.document) {
                alongside = Some(EntityError::Validation(EntityValidationReason::NoOverwriteConflict));
            }
            updated.append_attributes(&entry.document, OverwritePolicy::Preserve).map(drop)
        } else {
            updated
                .update_attributes(&entry.document)
                .and_then(|_| updated.append_attributes(&entry.document, OverwritePolicy::Overwrite))
                .map(drop)
        };
        if let Err(error) = applied {
            return Err(Box::new((entry.id, EntityError::from(error))));
        }

        // Batch update mirrors the single attribute update (clause 5.6.9):
        // the per-attribute triggers, partitioned from the before/after diff.
        let write = self.service.chunk_update(
            self.target,
            entry.id.clone(), // clone: the entry is kept to rebuild the write if the store refuses it
            UpdatedStates {
                previous,
                current: updated,
                addressed: AddressedInstances::EntityFragment {
                    fragment: &entry.document,
                    overwrite: self.overwrite,
                },
            },
            now,
        )?;
        Ok(PlannedWrite { entry, write, alongside })
    }
}

impl EntityService {
    pub(super) async fn batch_update_chunk(
        &self,
        placement: SideEffectPlacement<'_>,
        chunk: Vec<AdmittedBatchEntity>,
        overwrite: OverwritePolicy,
    ) -> Result<BatchResult, EntityError> {
        // Clause 5.5.4 excepts only the fragments "used in partial update and
        // merge operations" from its NGSI-LD Null prohibitions, and clause 5.2.1
        // makes every other placement of the sentinel `BadRequestData`. A batch
        // update is neither: clause 5.6.9.4's local phase names "the behaviour
        // defined by clause 5.6.3" -- Append Attributes, which carries no
        // NGSI-LD Null support of its own -- and clause 5.6.9 does not carry the
        // "NGSI-LD Nulls should be supported by this operation" sentence that
        // clauses 5.6.2, 5.6.4 and 5.6.17 do. The sentinel therefore has no
        // deletion semantics to be given here, and storing it would put
        // "urn:ngsi-ld:null" in a value the broker later serves back.
        //
        // What the code below runs is not clause 5.6.3 for the overwriting
        // default of clause 5.6.9.3: an overwriting update applies the clause
        // 5.6.2 patch first and appends what it left, and only `noOverwrite`
        // appends alone. That deviation is recorded in
        // `docs/spec-conformance-sweep.md`; it does not move the admission,
        // because clause 5.6.2's NGSI-LD Null support is a statement about the
        // Update Attributes operation and its Entity Fragment, not about a
        // whole Entity submitted to a batch.
        let (prepared, mut errors) = self.prepare_admitted_chunk(chunk, BatchWrite::Update).await;
        let rewrite = UpdateRewrite {
            service: self,
            target: placement.target(),
            overwrite,
        };
        let outcome = self.commit_chunk_read_modify_write(placement, &rewrite, prepared).await?;
        errors.extend(outcome.errors);
        Ok(BatchResult {
            success: outcome.success,
            errors,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::batch::update::appends_nothing;
    use andromeda_ngsi_ld_entity::{document::EntityDocument, entity_state::EntityState};
    use serde_json::{Value, json};

    const ENTITY_ID: &str = "urn:ngsi-ld:Vehicle:1";
    const BRAND_NAME: &str = "https://example.org/brandName";
    const IS_PARKED: &str = "https://example.org/isParked";
    const SPEED: &str = "https://example.org/speed";
    const SCOPE: &str = "https://uri.etsi.org/ngsi-ld/scope";

    fn property(value: &str) -> Value {
        json!([{"@type": ["https://uri.etsi.org/ngsi-ld/Property"], "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": value}]}])
    }

    fn relationship() -> Value {
        json!([{"@type": ["https://uri.etsi.org/ngsi-ld/Relationship"], "https://uri.etsi.org/ngsi-ld/hasObject": [{"@id": "urn:ngsi-ld:Parking:1"}]}])
    }

    /// An expanded node of `members` beside the entity's identity.
    fn node(members: &[(&str, Value)]) -> Value {
        let mut node = serde_json::Map::new();
        node.insert("@id".to_owned(), json!(ENTITY_ID));
        node.insert("@type".to_owned(), json!(["https://example.org/Vehicle"]));
        node.extend(members.iter().map(|(name, value)| ((*name).to_owned(), value.clone()))); // clone: the fixture keeps its members
        Value::Array(vec![Value::Object(node)])
    }

    fn stored() -> EntityState {
        EntityState::from_slice(node(&[(BRAND_NAME, property("x")), (IS_PARKED, relationship())]).to_string().as_bytes()).expect("a stored entity")
    }

    fn fragment(members: &[(&str, Value)]) -> EntityDocument {
        EntityDocument::from_slice(node(members).to_string().as_bytes()).expect("a written fragment")
    }

    /// Clause 5.6.9.5: an Entity is a success (S) only when at least one Attribute
    /// was appended. A fragment naming a not-yet-present Attribute appends it, even
    /// beside already-present ones; a fragment naming only present Attributes appends
    /// nothing.
    #[test]
    fn a_no_overwrite_fragment_appends_nothing_only_when_every_named_attribute_is_present() {
        let stored = stored();

        // Every named attribute already present -> appends nothing (207 path).
        assert!(appends_nothing(&stored, &fragment(&[(IS_PARKED, relationship())])));
        assert!(appends_nothing(&stored, &fragment(&[(BRAND_NAME, property("y")), (IS_PARKED, relationship())])));

        // At least one named attribute is new -> appends something (S / 204 path).
        assert!(!appends_nothing(&stored, &fragment(&[(SPEED, property("1"))])));
        assert!(!appends_nothing(&stored, &fragment(&[(IS_PARKED, relationship()), (SPEED, property("1"))])));

        // A fragment naming no member appends nothing new but is not a
        // noOverwrite conflict either.
        assert!(!appends_nothing(&stored, &fragment(&[])));
    }

    /// The scope is a member like any attribute (clause 4.18): a fragment
    /// naming it appends something only to an entity that holds none.
    #[test]
    fn a_no_overwrite_scope_appends_nothing_only_to_an_entity_holding_a_scope() {
        let scoped = fragment(&[(SCOPE, json!([{"@value": "/Madrid"}]))]);
        assert!(!appends_nothing(&stored(), &scoped));

        let holding = EntityState::from_slice(node(&[(SCOPE, json!([{"@value": "/Berlin"}]))]).to_string().as_bytes()).expect("a stored entity");
        assert!(appends_nothing(&holding, &scoped));
    }
}
