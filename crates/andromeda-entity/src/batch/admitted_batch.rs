//! The entities of a batch write the clause 5.5.4 admission let through, each
//! with the document its preparation expanded.
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
    batch::{
        context_groups::{ExpandedEntity, PreparedBatch},
        prepare::{PendingEntity, PreparedEntity},
    },
    error::EntityError,
    write_admission::BatchWrite,
    write_kind::BatchWriteKind,
};
use andromeda_jsonld::batch_contexts::{BatchContext, ContextGroups};
use andromeda_ngsi_ld_entity::document::EntityDocument;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
use andromeda_ngsi_ld_model::{attribute_strip::AttributeStrip, entity::Entity};

/// A batch entity as submitted, with the context it is read against.
pub struct SubmittedEntity {
    /// The decoded entity.
    pub entity: Entity,
    /// The context the entity is read against: its own `@context` in an
    /// `application/ld+json` batch, the request's `Link` context otherwise
    /// (clause 5.5.7). Entities naming the same context share one identity.
    pub context: BatchContext,
}

/// Where an admitted entity's local write document comes from.
///
/// The prepared document is boxed because it is an order of magnitude larger
/// than the marker of a narrowed entity, and admitted entities are moved
/// through the rounds and chunks of the local phase.
enum Preparation {
    /// The document the admission prepared, still describing the entity.
    Prepared(Box<EntityDocument>),
    /// The forwarding phase narrowed the entity, so its document is prepared
    /// again from what remains.
    Narrowed,
}

/// One entity the admission let through.
pub(super) struct AdmittedBatchEntity {
    /// The entity as it stands, forwarded as it is and narrowed in place.
    entity: Entity,
    /// Its local write document.
    preparation: Preparation,
    /// The context the entity was submitted with, which a narrowed entity is
    /// prepared again under.
    context: BatchContext,
}

impl AdmittedBatchEntity {
    /// The entity's identifier.
    pub(super) const fn id(&self) -> &Identifier {
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

    /// The admitted entities, as the forwarding phase sends them, each with
    /// the context it was submitted under (a top-level node's `@context`
    /// applies to that node alone).
    pub fn entities(&self) -> impl Iterator<Item = (&Entity, &BatchContext)> {
        self.entities.iter().map(|admitted| (&admitted.entity, &admitted.context))
    }

    /// Keep what the local phase processes once proxied registrations took
    /// their attributes (clauses 5.6.7.4 and following: "any remaining input
    /// data"): each entity without the attributes `proxied_for` names for it,
    /// and none of the entities left without an attribute.
    pub fn retain_local(&mut self, mut proxied_for: impl FnMut(&Identifier) -> Vec<AttributeName>) {
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
    /// Run the clause 5.5.4 screening of `kind`, and the payload budget, over
    /// a batch's submitted entities, before the operation forwards any of them, and keep what it
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
    pub async fn admit_batch_write<W: BatchWriteKind>(&self, kind: W, submitted: Vec<SubmittedEntity>) -> (AdmittedBatch<W>, Vec<(Identifier, EntityError)>) {
        let slot_count = submitted.len();
        let mut groups = ContextGroups::default();
        for (idx, SubmittedEntity { entity, context }) in submitted.into_iter().enumerate() {
            groups.push(&context, PendingEntity { idx, entity });
        }
        let PreparedBatch { slots, errors } = self.prepare_grouped(slot_count, groups, kind.write()).await;
        let mut admitted = slots
            .into_iter()
            .flatten()
            .map(|ExpandedEntity { entity, document, context }| AdmittedBatchEntity {
                entity,
                preparation: Preparation::Prepared(Box::new(document)),
                context,
            })
            .collect::<Vec<_>>();
        for (id, _) in &errors {
            admitted.retain(|entity| entity.entity.id != *id);
        }
        (AdmittedBatch { kind, entities: admitted }, errors)
    }

    /// The local write documents of one chunk of admitted entities, in chunk
    /// order.
    ///
    /// An entity the forwarding phase left untouched is written as the
    /// admission prepared it; a narrowed one is prepared again for the batch
    /// operation `write`, against the context it was submitted with, and a
    /// failure of that is reported against its identifier.
    pub(super) async fn prepare_admitted_chunk(
        &self,
        chunk: Vec<AdmittedBatchEntity>,
        write: BatchWrite,
    ) -> (Vec<PreparedEntity>, Vec<(Identifier, EntityError)>) {
        let mut slots: Vec<Option<PreparedEntity>> = Vec::with_capacity(chunk.len());
        let mut groups = ContextGroups::default();
        let mut narrowed = 0;
        for AdmittedBatchEntity { entity, preparation, context } in chunk {
            match preparation {
                Preparation::Prepared(document) => slots.push(Some(PreparedEntity {
                    id: entity.id,
                    document: *document,
                })),
                Preparation::Narrowed => {
                    slots.push(None);
                    groups.push(&context, PendingEntity { idx: narrowed, entity });
                    narrowed += 1;
                }
            }
        }
        if narrowed == 0 {
            return (slots.into_iter().flatten().collect(), Vec::new());
        }
        let PreparedBatch { slots: fresh, errors } = self.prepare_grouped(narrowed, groups, write).await;
        let mut fresh = fresh.into_iter();
        let prepared = slots
            .into_iter()
            .filter_map(|slot| match slot {
                Some(prepared) => Some(prepared),
                None => fresh
                    .next()
                    .flatten()
                    .map(|ExpandedEntity { entity, document, .. }| PreparedEntity { id: entity.id, document }),
            })
            .collect();
        (prepared, errors)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        EntityService,
        admission_fixture::{admitted_batch, submitted},
        batch::{
            UpsertPolicy,
            admitted_batch::{AdmittedBatch, Preparation, SubmittedEntity},
            prepare::PreparedEntity,
        },
        error::EntityError,
        notification_recording_harness::{budgeted_recording_service, geometry_budget, recording_service},
        write_admission::BatchWrite,
        write_kind::{BatchCreate, BatchUpsert},
    };
    use andromeda_jsonld::{
        batch_contexts::{BatchContext, BatchContexts},
        context_spec::ContextSpec,
    };
    use andromeda_ngsi_ld_entity::document::EntityDocument;
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
    use andromeda_ngsi_ld_model::entity::Entity;
    use andromeda_ngsi_ld_representation::payload_budget::PayloadBudgetExceeded;
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

    /// The expanded attribute names a document writes, in IRI order.
    fn attribute_iris(document: &EntityDocument) -> Vec<String> {
        let mut iris: Vec<String> = document.attributes().keys().map(AttributeName::to_string).collect();
        iris.sort_unstable();
        iris
    }

    /// The expanded attribute names of each prepared entity, by identifier.
    fn prepared_iris(prepared: &[PreparedEntity]) -> Vec<(String, Vec<String>)> {
        prepared
            .iter()
            .map(|entity| (entity.id.to_string(), attribute_iris(&entity.document)))
            .collect()
    }

    /// Which entities the batch holds, in order, and whether each still carries
    /// the document its admission prepared.
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
    fn proxied(id: &Identifier) -> Vec<AttributeName> {
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

    const SPEED_IRI: &str = "https://uri.etsi.org/ngsi-ld/default-context/speed";
    const BRAND_NAME_IRI: &str = "https://uri.etsi.org/ngsi-ld/default-context/brandName";

    /// Clauses 5.6.7.4 and following: the local phase runs on "any remaining
    /// input data". Only an entity the strip actually changed loses the
    /// document its admission prepared; one the strip left alone keeps it.
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
        let (prepared, errors) = service.prepare_admitted_chunk(entities, BatchWrite::Create).await;
        assert!(errors.is_empty(), "{errors:?}");
        pretty_assertions::assert_eq!(
            prepared_iris(&prepared),
            vec![
                ("urn:ngsi-ld:Vehicle:1".to_owned(), vec![BRAND_NAME_IRI.to_owned()]),
                ("urn:ngsi-ld:Vehicle:3".to_owned(), vec![BRAND_NAME_IRI.to_owned(), SPEED_IRI.to_owned()]),
                ("urn:ngsi-ld:Vehicle:4".to_owned(), vec![BRAND_NAME_IRI.to_owned(), SPEED_IRI.to_owned()]),
            ]
        );
    }

    /// A narrowed entity is screened again when it is prepared again, and a
    /// failure is reported against its own identifier while the rest of the
    /// chunk is written. The update mode of an upsert admits an NGSI-LD Null
    /// value; preparing the remainder as a creation, which refuses it, shows
    /// the remainder is screened in its own right.
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
        let (prepared, errors) = service.prepare_admitted_chunk(entities, BatchWrite::Create).await;
        pretty_assertions::assert_eq!(
            prepared.iter().map(|entity| entity.id.to_string()).collect::<Vec<_>>(),
            vec!["urn:ngsi-ld:Vehicle:2"]
        );
        pretty_assertions::assert_eq!(errors.iter().map(|(id, _)| id.to_string()).collect::<Vec<_>>(), vec!["urn:ngsi-ld:Vehicle:1"]);
        assert!(errors[0].1.is_validation(), "{}", errors[0].1);
    }

    /// A context mapping `term` to `iri`, as an item of an
    /// `application/ld+json` batch carries it, interned in `contexts`.
    fn term_context(contexts: &mut BatchContexts, term: &str, iri: &str) -> BatchContext {
        contexts.context_of(Some(json!({ term: iri })))
    }

    /// The expanded attribute names the admission prepared, by identifier.
    fn expanded_by_id(admitted: AdmittedBatch<BatchCreate>) -> Vec<(String, Vec<String>)> {
        let (_, entities) = admitted.into_parts();
        entities
            .into_iter()
            .map(|admitted| match admitted.preparation {
                Preparation::Prepared(document) => (admitted.entity.id.to_string(), attribute_iris(&document)),
                Preparation::Narrowed => (admitted.entity.id.to_string(), Vec::new()),
            })
            .collect()
    }

    fn submitted_under(id: &str, context: &BatchContext) -> SubmittedEntity {
        SubmittedEntity {
            entity: vehicle(id),
            context: context.clone(), // clone: each entity holds the shared handle
        }
    }

    /// JSON-LD applies a top-level node's `@context` to that node alone: two
    /// entities of one batch naming `speed` through different contexts store
    /// two different attribute IRIs, and neither reads the other's definition.
    #[tokio::test]
    async fn each_entity_of_a_batch_is_expanded_under_its_own_context() {
        let (service, _recorded) = recording_service().await;
        let mut contexts = BatchContexts::per_entity();
        let first = term_context(&mut contexts, "speed", "https://a.example/speed");
        let second = term_context(&mut contexts, "speed", "https://b.example/speed");
        let submitted = vec![
            submitted_under("urn:ngsi-ld:Vehicle:1", &first),
            submitted_under("urn:ngsi-ld:Vehicle:2", &second),
            submitted_under("urn:ngsi-ld:Vehicle:3", &first),
        ];

        let (admitted, refused) = service.admit_batch_write(BatchCreate, submitted).await;

        assert!(refused.is_empty(), "{refused:?}");
        pretty_assertions::assert_eq!(
            expanded_by_id(admitted),
            vec![
                (
                    "urn:ngsi-ld:Vehicle:1".to_owned(),
                    vec!["https://a.example/speed".to_owned(), BRAND_NAME_IRI.to_owned()]
                ),
                (
                    "urn:ngsi-ld:Vehicle:2".to_owned(),
                    vec!["https://b.example/speed".to_owned(), BRAND_NAME_IRI.to_owned()]
                ),
                (
                    "urn:ngsi-ld:Vehicle:3".to_owned(),
                    vec!["https://a.example/speed".to_owned(), BRAND_NAME_IRI.to_owned()]
                ),
            ],
            "entities keep their submission order across contexts"
        );
    }

    /// A narrowed entity is prepared again under the context it was submitted
    /// with, not under another entity's.
    #[tokio::test]
    async fn a_narrowed_entity_is_prepared_again_under_its_own_context() {
        let (service, _recorded) = recording_service().await;
        let mut contexts = BatchContexts::per_entity();
        let own = term_context(&mut contexts, "brandName", "https://a.example/brandName");
        let other = term_context(&mut contexts, "brandName", "https://b.example/brandName");
        let submitted = vec![submitted_under("urn:ngsi-ld:Vehicle:1", &own), submitted_under("urn:ngsi-ld:Vehicle:2", &other)];
        let (mut admitted, refused) = service.admit_batch_write(BatchCreate, submitted).await;
        assert!(refused.is_empty(), "{refused:?}");
        admitted.retain_local(|id| {
            if id.as_str() == "urn:ngsi-ld:Vehicle:1" {
                names(&["speed"])
            } else {
                Vec::new()
            }
        });

        let (_, entities) = admitted.into_parts();
        let (prepared, errors) = service.prepare_admitted_chunk(entities, BatchWrite::Create).await;

        assert!(errors.is_empty(), "{errors:?}");
        pretty_assertions::assert_eq!(
            prepared_iris(&prepared),
            vec![
                ("urn:ngsi-ld:Vehicle:1".to_owned(), vec!["https://a.example/brandName".to_owned()]),
                (
                    "urn:ngsi-ld:Vehicle:2".to_owned(),
                    vec!["https://b.example/brandName".to_owned(), SPEED_IRI.to_owned()]
                ),
            ]
        );
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
        let (admitted, refused) = service.admit_batch_write(BatchCreate, submitted(entities, &ContextSpec::core_only())).await;
        pretty_assertions::assert_eq!(refused.iter().map(|(id, _)| id.to_string()).collect::<Vec<_>>(), vec!["urn:ngsi-ld:Vehicle:1"]);
        pretty_assertions::assert_eq!(
            admitted.entities().map(|(entity, _)| entity.id.to_string()).collect::<Vec<_>>(),
            vec!["urn:ngsi-ld:Vehicle:2"]
        );
    }

    /// Clause 5.6.7.4 reports a failed entity as one `BatchEntityError` rather
    /// than failing the request: an entity over the payload budget is refused
    /// against its own identifier, and its neighbour is admitted.
    #[tokio::test]
    async fn the_batch_admission_refuses_only_the_oversized_entity() {
        let service = budgeted_recording_service(geometry_budget(4));
        let located = entity(&json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "location": {"type": "GeoProperty", "value": {"type": "Polygon", "coordinates": [[[0, 0], [1, 0], [1, 1], [0, 1], [0, 0]]]}}
        }));
        let entities = vec![located, vehicle("urn:ngsi-ld:Vehicle:2")];

        let (admitted, refused) = service.admit_batch_write(BatchCreate, submitted(entities, &ContextSpec::core_only())).await;

        let [(id, refusal)] = refused.as_slice() else {
            panic!("exactly the oversized entity is refused: {refused:?}");
        };
        pretty_assertions::assert_eq!(id.as_str(), "urn:ngsi-ld:Vehicle:1");
        assert!(
            matches!(refusal, EntityError::PayloadBudget(PayloadBudgetExceeded::GeometryPositions { .. })),
            "{refusal:?}"
        );
        pretty_assertions::assert_eq!(
            admitted.entities().map(|(entity, _)| entity.id.to_string()).collect::<Vec<_>>(),
            vec!["urn:ngsi-ld:Vehicle:2"]
        );
    }

    /// Clause 5.6.7.4 again: an entity nesting an attribute past the limit is
    /// refused against its own identifier, as the `BadRequestData` a single
    /// write answers it with, and its neighbour is admitted.
    #[tokio::test]
    async fn the_batch_admission_refuses_only_the_entity_nesting_too_deeply() {
        let (service, _recorded) = recording_service().await;
        let deep = (0..33).fold(json!("leaf"), |inner, _| json!({"k": inner}));
        let nested = entity(&json!({"id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle", "reading": {"type": "Property", "value": deep}}));
        let entities = vec![nested, vehicle("urn:ngsi-ld:Vehicle:2")];

        let (admitted, refused) = service.admit_batch_write(BatchCreate, submitted(entities, &ContextSpec::core_only())).await;

        let [(id, refusal)] = refused.as_slice() else {
            panic!("exactly the nesting entity is refused: {refused:?}");
        };
        pretty_assertions::assert_eq!(id.as_str(), "urn:ngsi-ld:Vehicle:1");
        let EntityError::AttributeNesting(exceeded) = refusal else {
            panic!("refused for its nesting: {refusal:?}");
        };
        pretty_assertions::assert_eq!(exceeded.attribute.as_str(), "https://uri.etsi.org/ngsi-ld/default-context/reading");
        pretty_assertions::assert_eq!(exceeded.limit.get(), 32);
        pretty_assertions::assert_eq!(
            admitted.entities().map(|(entity, _)| entity.id.to_string()).collect::<Vec<_>>(),
            vec!["urn:ngsi-ld:Vehicle:2"]
        );
    }
}
