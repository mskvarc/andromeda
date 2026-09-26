//! The entities of a batch write the clause 5.5.4 admission let through, each
//! with the payload its preparation expanded.
//!
//! Clauses 5.6.7.4 to 5.6.9.4 and 5.6.20.4 each open with "execute the
//! behaviour defined in clause 5.5.4 on JSON-LD validation" and only then
//! forward to matching registrations. The admission prepares every entity the
//! way the local phase writes it, and the local phase writes what it
//! prepared: an entity is expanded once per request. Only an entity the
//! forwarding phase narrowed -- some of its attributes claimed by an exclusive
//! or redirect registration (clause 4.3.6.3) -- is prepared again, because what
//! remains of it is a different document.

use crate::{
    EntityService,
    batch::prepare::{ClassifiedEntry, PreparedBatch, PreparedEntity, render_entries},
    error::EntityError,
    null_sentinel_admission::NullSentinelAdmission,
    write_kind::BatchWriteKind,
};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
use andromeda_ngsi_ld_model::{attribute_strip::AttributeStrip, entity::Entity};
use serde_json::Value;

/// A batch entity as submitted, with the compact JSON the admission renders it
/// to for the rules decided before expansion.
pub struct RenderedEntity {
    /// The decoded entity.
    pub entity: Entity,
    /// The entity rendered back to compact JSON.
    pub representation: Value,
}

/// Where an admitted entity's local write payload comes from.
///
/// The prepared payload is boxed because it is an order of magnitude larger
/// than the marker of a narrowed entity, and admitted entities are moved
/// through the rounds and chunks of the local phase.
enum Preparation {
    /// The payload the admission prepared, still describing the entity.
    Prepared(Box<PreparedEntity>),
    /// The forwarding phase narrowed the entity, so its payload is prepared
    /// again from what remains.
    Narrowed,
}

/// One entity the admission let through.
pub(super) struct AdmittedBatchEntity {
    /// The entity as it stands, forwarded as it is and narrowed in place.
    entity: Entity,
    /// Its local write payload.
    preparation: Preparation,
}

impl AdmittedBatchEntity {
    /// The entity's identifier.
    pub(super) const fn id(&self) -> &Uri {
        &self.entity.id
    }
}

/// The entities of a batch write `W` the clause 5.5.4 admission let through,
/// in submission order.
///
/// Only [`EntityService::admit_batch_write`] constructs a non-empty one, so
/// holding one proves every entity in it passed the screening of `W`.
pub struct AdmittedBatch<W> {
    /// The operation, in the mode the request selected.
    kind: W,
    /// The admitted entities, in submission order.
    entities: Vec<AdmittedBatchEntity>,
}

impl<W: BatchWriteKind> AdmittedBatch<W> {
    /// A batch with nothing admitted, for a request whose every entity was
    /// refused before any was prepared.
    #[must_use]
    pub const fn empty(kind: W) -> AdmittedBatch<W> {
        AdmittedBatch { kind, entities: Vec::new() }
    }

    /// Whether no entity was admitted.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }

    /// How many entities were admitted.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.entities.len()
    }

    /// The admitted entities, as the forwarding phase sends them.
    pub fn entities(&self) -> impl Iterator<Item = &Entity> {
        self.entities.iter().map(|admitted| &admitted.entity)
    }

    /// Keep what the local phase processes once proxied registrations took
    /// their attributes (clauses 5.6.7.4 and following: "any remaining input
    /// data"): each entity without the attributes `proxied_for` names for it,
    /// and none of the entities left without an attribute.
    pub fn retain_local(&mut self, mut proxied_for: impl FnMut(&Uri) -> Vec<AttributeName>) {
        self.entities.retain_mut(|admitted| {
            let proxied = proxied_for(&admitted.entity.id);
            if proxied.is_empty() {
                return true;
            }
            match admitted.entity.strip_attributes(&proxied) {
                AttributeStrip::Untouched => true,
                AttributeStrip::Narrowed => {
                    admitted.preparation = Preparation::Narrowed;
                    true
                }
                AttributeStrip::Emptied => false,
            }
        });
    }

    /// The operation and the admitted entities, for the local phase.
    pub(super) fn into_parts(self) -> (W, Vec<AdmittedBatchEntity>) {
        (self.kind, self.entities)
    }
}

impl EntityService {
    /// Run the clause 5.5.4 screening of `kind` over a batch's rendered
    /// entities, before the operation forwards any of them, and keep what it
    /// prepared for the local phase.
    ///
    /// An entity this broker refuses is never handed to a Context Source, and
    /// one whose attributes a proxied registration claims in full -- an entity
    /// the local phase never sees -- has still been validated.
    ///
    /// Returns, beside the admitted entities, one error per entity that fails,
    /// against that entity's own identifier: clause 5.6.7.4 adds "a new
    /// `BatchEntityError` [...] containing the failed Entity ID and the related
    /// `ProblemDetails`" rather than failing the request. An identifier that
    /// fails is refused for every entity carrying it.
    pub async fn admit_batch_write<W: BatchWriteKind>(
        &self,
        kind: W,
        rendered: Vec<RenderedEntity>,
        context: &ContextSpec,
    ) -> (AdmittedBatch<W>, Vec<(Uri, EntityError)>) {
        let slot_count = rendered.len();
        let (entities, entries): (Vec<Entity>, Vec<ClassifiedEntry>) = rendered
            .into_iter()
            .enumerate()
            .map(|(idx, RenderedEntity { entity, representation })| {
                let id = entity.id.clone(); // clone: the classified entry owns the identifier its failure is reported against
                (
                    entity,
                    ClassifiedEntry {
                        idx,
                        id,
                        value: representation,
                    },
                )
            })
            .unzip();
        let PreparedBatch { slots, errors } = self
            .prepare_payloads(slot_count, entries, context, kind.write().null_sentinel_admission())
            .await;
        let mut admitted = entities
            .into_iter()
            .zip(slots)
            .filter_map(|(entity, slot)| {
                slot.map(|prepared| AdmittedBatchEntity {
                    entity,
                    preparation: Preparation::Prepared(Box::new(prepared)),
                })
            })
            .collect::<Vec<_>>();
        for (id, _) in &errors {
            admitted.retain(|entity| entity.entity.id != *id);
        }
        (AdmittedBatch { kind, entities: admitted }, errors)
    }

    /// The local write payloads of one chunk of admitted entities, in chunk
    /// order.
    ///
    /// An entity the forwarding phase left untouched is written as the
    /// admission prepared it; a narrowed one is rendered and prepared again
    /// under `admission`, and a failure of that is reported against its
    /// identifier.
    pub(super) async fn prepare_admitted_chunk(
        &self,
        chunk: Vec<AdmittedBatchEntity>,
        context: &ContextSpec,
        admission: NullSentinelAdmission,
    ) -> (Vec<PreparedEntity>, Vec<(Uri, EntityError)>) {
        let mut slots: Vec<Option<PreparedEntity>> = Vec::with_capacity(chunk.len());
        let mut narrowed: Vec<Entity> = Vec::new();
        for AdmittedBatchEntity { entity, preparation } in chunk {
            match preparation {
                Preparation::Prepared(prepared) => slots.push(Some(*prepared)),
                Preparation::Narrowed => {
                    slots.push(None);
                    narrowed.push(entity);
                }
            }
        }
        if narrowed.is_empty() {
            return (slots.into_iter().flatten().collect(), Vec::new());
        }
        let (entries, mut errors) = render_entries(&narrowed);
        let PreparedBatch {
            slots: fresh,
            errors: preparation_errors,
        } = self.prepare_payloads(narrowed.len(), entries, context, admission).await;
        errors.extend(preparation_errors);
        let mut fresh = fresh.into_iter();
        let prepared = slots
            .into_iter()
            .filter_map(|slot| match slot {
                Some(prepared) => Some(prepared),
                None => fresh.next().flatten(),
            })
            .collect();
        (prepared, errors)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        EntityService,
        admission_fixture::{admitted_batch, rendered},
        batch::{
            UpsertPolicy,
            admitted_batch::{AdmittedBatch, Preparation},
        },
        error::EntityError,
        notification_recording_harness::recording_service,
        null_sentinel_admission::NullSentinelAdmission,
        write_admission::BatchWrite,
        write_kind::{BatchCreate, BatchUpsert},
    };
    use andromeda_jsonld::context_spec::ContextSpec;
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
    use andromeda_ngsi_ld_model::entity::Entity;
    use serde_json::{Value, json};

    /// A Vehicle carrying `speed` and `brandName`, as a client writes it.
    fn vehicle(id: &str) -> Entity {
        entity(&json!({
            "id": id,
            "type": "Vehicle",
            "speed": {"type": "Property", "value": 1},
            "brandName": {"type": "Property", "value": "x"}
        }))
    }

    fn entity(written: &Value) -> Entity {
        serde_json::from_value(written.clone()).expect("the fixture is a valid entity") // clone: the fixture keeps its written form
    }

    fn names(raw: &[&str]) -> Vec<AttributeName> {
        raw.iter().map(|name| name.parse().expect("valid attribute name")).collect()
    }

    /// Which entities the batch holds, in order, and whether each still carries
    /// the payload its admission prepared.
    fn preparations(admitted: &AdmittedBatch<BatchCreate>) -> Vec<(String, &'static str)> {
        admitted
            .entities
            .iter()
            .map(|entity| {
                let preparation = match entity.preparation {
                    Preparation::Prepared(_) => "prepared",
                    Preparation::Narrowed => "narrowed",
                };
                (entity.entity.id.to_string(), preparation)
            })
            .collect()
    }

    /// The narrowing a batch goes through once its forwarding phase answered:
    /// V1 loses `speed`, V2 loses every attribute, V3 is claimed by nothing, and
    /// V4 is claimed only for an attribute it does not carry.
    fn proxied(id: &Uri) -> Vec<AttributeName> {
        match id.as_str() {
            "urn:ngsi-ld:Vehicle:1" => names(&["speed"]),
            "urn:ngsi-ld:Vehicle:2" => names(&["speed", "brandName"]),
            "urn:ngsi-ld:Vehicle:4" => names(&["colour"]),
            _ => Vec::new(),
        }
    }

    async fn narrowed_batch() -> (EntityService, AdmittedBatch<BatchCreate>) {
        let (service, _recorded) = recording_service().await;
        let entities = [
            "urn:ngsi-ld:Vehicle:1",
            "urn:ngsi-ld:Vehicle:2",
            "urn:ngsi-ld:Vehicle:3",
            "urn:ngsi-ld:Vehicle:4",
        ]
        .map(vehicle);
        let mut admitted = admitted_batch(&service, BatchCreate, Vec::from(entities), &ContextSpec::core_only()).await;
        admitted.retain_local(proxied);
        (service, admitted)
    }

    /// Clauses 5.6.7.4 and following: the local phase runs on "any remaining
    /// input data". Only an entity the strip actually changed loses the
    /// payload its admission prepared; one the strip left alone keeps it.
    #[tokio::test]
    async fn only_an_entity_the_forwarding_phase_narrowed_is_prepared_again() {
        let (_service, admitted) = narrowed_batch().await;
        pretty_assertions::assert_eq!(
            preparations(&admitted),
            vec![
                ("urn:ngsi-ld:Vehicle:1".to_owned(), "narrowed"),
                ("urn:ngsi-ld:Vehicle:3".to_owned(), "prepared"),
                ("urn:ngsi-ld:Vehicle:4".to_owned(), "prepared"),
            ]
        );
    }

    /// The local phase writes the chunk in submission order whichever entities
    /// it prepares again, and what it prepares again is what remains.
    #[tokio::test]
    async fn a_chunk_is_prepared_in_submission_order_with_the_narrowed_entity_reduced_to_what_remains() {
        let (service, admitted) = narrowed_batch().await;
        let (_, entities) = admitted.into_parts();
        let (prepared, errors) = service
            .prepare_admitted_chunk(entities, &ContextSpec::core_only(), BatchWrite::Create.null_sentinel_admission())
            .await;
        assert!(errors.is_empty(), "{errors:?}");
        pretty_assertions::assert_eq!(
            prepared.iter().map(|entity| entity.id.to_string()).collect::<Vec<_>>(),
            vec!["urn:ngsi-ld:Vehicle:1", "urn:ngsi-ld:Vehicle:3", "urn:ngsi-ld:Vehicle:4"]
        );
        let narrowed = prepared[0].expanded.to_string();
        assert!(!narrowed.contains("/speed") && narrowed.contains("/brandName"), "{narrowed}");
        assert!(prepared[1].expanded.to_string().contains("/speed"));
    }

    /// A narrowed entity is screened again when it is prepared again, and a
    /// failure is reported against its own identifier while the rest of the
    /// chunk is written. The update mode of an upsert admits an NGSI-LD Null
    /// value; preparing the remainder under the refusing admission of a
    /// creation shows the remainder is screened in its own right.
    #[tokio::test]
    async fn a_narrowed_entity_that_fails_its_preparation_is_reported_against_its_identifier() {
        let (service, _recorded) = recording_service().await;
        let deleting = entity(&json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "speed": {"type": "Property", "value": 1},
            "brandName": {"type": "Property", "value": "urn:ngsi-ld:null"}
        }));
        let entities = vec![deleting, vehicle("urn:ngsi-ld:Vehicle:2")];
        let mut admitted = admitted_batch(&service, BatchUpsert(UpsertPolicy::Update), entities, &ContextSpec::core_only()).await;
        admitted.retain_local(|id| {
            if id.as_str() == "urn:ngsi-ld:Vehicle:1" {
                names(&["speed"])
            } else {
                Vec::new()
            }
        });
        let (_, entities) = admitted.into_parts();
        let (prepared, errors) = service
            .prepare_admitted_chunk(entities, &ContextSpec::core_only(), NullSentinelAdmission::Refused)
            .await;
        pretty_assertions::assert_eq!(
            prepared.iter().map(|entity| entity.id.to_string()).collect::<Vec<_>>(),
            vec!["urn:ngsi-ld:Vehicle:2"]
        );
        pretty_assertions::assert_eq!(errors.iter().map(|(id, _)| id.to_string()).collect::<Vec<_>>(), vec!["urn:ngsi-ld:Vehicle:1"]);
        assert!(matches!(errors[0].1, EntityError::Validation(_)), "{}", errors[0].1);
    }

    /// An identifier the screening refuses is refused for every entity carrying
    /// it, the way the batch was narrowed before the admission kept what it
    /// prepared.
    #[tokio::test]
    async fn a_refused_identifier_is_refused_for_every_entity_carrying_it() {
        let (service, _recorded) = recording_service().await;
        let entities = vec![
            vehicle("urn:ngsi-ld:Vehicle:1"),
            entity(&json!({"id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle", "speed": {"type": "Property", "value": "urn:ngsi-ld:null"}})),
            vehicle("urn:ngsi-ld:Vehicle:2"),
        ];
        let (admitted, refused) = service.admit_batch_write(BatchCreate, rendered(entities), &ContextSpec::core_only()).await;
        pretty_assertions::assert_eq!(refused.iter().map(|(id, _)| id.to_string()).collect::<Vec<_>>(), vec!["urn:ngsi-ld:Vehicle:1"]);
        pretty_assertions::assert_eq!(
            admitted.entities().map(|entity| entity.id.to_string()).collect::<Vec<_>>(),
            vec!["urn:ngsi-ld:Vehicle:2"]
        );
    }
}
