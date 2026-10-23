//! The entities of a batch write the clause 5.5.4 admission let through, each
//! with the document its preparation expanded.
//!
//! Clauses 5.6.7.4 to 5.6.9.4 and 5.6.20.4 each open with "execute the
//! behaviour defined in clause 5.5.4 on JSON-LD validation" and only then
//! forward to matching registrations. The admission prepares every entity the
//! way the local phase writes it, and the local phase writes what it
//! prepared: an entity is expanded once per request. The forwarding phase
//! reads the same documents, and what it leaves to the local phase, each
//! document without the attributes an exclusive or redirect registration
//! claimed (clause 4.3.6.3), is a part of what was admitted, so it is never
//! prepared again.

use crate::{
    EntityService,
    batch::{
        context_groups::{ExpandedEntity, PreparedBatch},
        prepare::{PendingEntity, PreparedEntity},
    },
    error::EntityError,
    write_kind::BatchWriteKind,
};
use andromeda_jsonld::batch_contexts::{AdmittedBatchContext, BatchContext, ContextGroups};
use andromeda_ngsi_ld_entity::document::EntityDocument;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
use andromeda_ngsi_ld_model::{attribute_strip::AttributeStrip, entity::Entity};

/// A batch entity as submitted, with the context it is read against.
pub struct SubmittedEntity<M> {
    /// The decoded entity.
    pub entity: Entity<M>,
    /// The context the entity is read against: its own `@context` in an
    /// `application/ld+json` batch, the request's `Link` context otherwise
    /// (clause 5.5.7), admitted before the entity is. Entities naming the same
    /// context share one identity.
    pub context: AdmittedBatchContext,
}

/// One entity the admission let through.
pub(super) struct AdmittedBatchEntity {
    /// The entity's identifier.
    id: Identifier,
    /// Its local write document: the one the admission prepared, narrowed in
    /// place when proxied registrations take attributes. Boxed because the
    /// admitted entities are moved through the rounds and chunks of the local
    /// phase.
    document: Box<EntityDocument>,
    /// The context the entity was submitted with, which the forwarding phase
    /// spells the entity back from.
    context: BatchContext,
}

impl AdmittedBatchEntity {
    /// The entity's identifier.
    pub(super) const fn id(&self) -> &Identifier {
        &self.id
    }

    /// The local write document the entity is written from.
    fn into_prepared(self) -> PreparedEntity {
        PreparedEntity {
            id: self.id,
            document: *self.document,
        }
    }
}

/// One admitted entity as the forwarding phase reads it.
#[derive(Clone, Copy, Debug)]
pub struct ForwardableEntity<'a> {
    /// The entity's identifier.
    pub id: &'a Identifier,
    /// The admitted document, in expanded names.
    pub document: &'a EntityDocument,
    /// The context the entity was submitted under (a top-level node's
    /// `@context` applies to that node alone).
    pub context: &'a BatchContext,
}

/// The entities of a batch write `W` the clause 5.5.4 admission let through,
/// in submission order.
///
/// Only [`EntityService::admit_batch_write`] constructs a non-empty one, so
/// holding one proves every entity in it passed the screening of `W`.
pub struct AdmittedBatch<W: BatchWriteKind> {
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

    /// The admitted entities, as the forwarding phase reads them, in
    /// submission order.
    pub fn forwardable(&self) -> impl Iterator<Item = ForwardableEntity<'_>> {
        self.entities.iter().map(|admitted| ForwardableEntity {
            id: &admitted.id,
            document: &admitted.document,
            context: &admitted.context,
        })
    }

    /// Keep what the local phase processes once proxied registrations took
    /// their attributes (clauses 5.6.7.4 and following: "any remaining input
    /// data"): each entity without the attributes `proxied_for` names for it
    /// by their expanded names, and none of the entities left without an
    /// attribute.
    pub fn retain_local(&mut self, mut proxied_for: impl FnMut(&Identifier) -> Vec<AttributeName>) {
        self.entities.retain_mut(|admitted| {
            let proxied = proxied_for(&admitted.id);
            if proxied.is_empty() {
                return true;
            }
            let before = admitted.document.attributes().len();
            admitted.document.attributes_mut().retain(|name, _| !proxied.contains(name));
            match AttributeStrip::from_counts(before, admitted.document.attributes().len()) {
                AttributeStrip::Emptied => false,
                AttributeStrip::Untouched | AttributeStrip::Narrowed => true,
            }
        });
    }

    /// The operation and the admitted entities, for the local phase.
    pub(super) fn into_parts(self) -> (W, Vec<AdmittedBatchEntity>) {
        (self.kind, self.entities)
    }
}

/// The local write documents of one chunk of admitted entities, in chunk
/// order.
pub(super) fn prepared_chunk(chunk: Vec<AdmittedBatchEntity>) -> Vec<PreparedEntity> {
    chunk.into_iter().map(AdmittedBatchEntity::into_prepared).collect()
}

impl EntityService {
    /// Run the clause 5.5.4 screening of `kind`, and the payload budget, over
    /// a batch's submitted entities, before the operation forwards any of them, and keep what it
    /// prepared for the local phase.
    ///
    /// An entity this broker refuses is never handed to a Context Source, and
    /// one whose attributes a proxied registration claims in full, an entity
    /// the local phase never sees, has still been validated.
    ///
    /// Returns, beside the admitted entities, one error per entity that fails,
    /// against that entity's own identifier: clause 5.6.7.4 adds "a new
    /// `BatchEntityError` [...] containing the failed Entity ID and the related
    /// `ProblemDetails`" rather than failing the request. An identifier that
    /// fails is refused for every entity carrying it.
    pub async fn admit_batch_write<W: BatchWriteKind>(
        &self,
        kind: W,
        submitted: Vec<SubmittedEntity<W::Member>>,
    ) -> (AdmittedBatch<W>, Vec<(Identifier, EntityError)>) {
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
                id: entity.id,
                document: Box::new(document),
                context,
            })
            .collect::<Vec<_>>();
        for (id, _) in &errors {
            admitted.retain(|entity| entity.id != *id);
        }
        (AdmittedBatch { kind, entities: admitted }, errors)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        EntityService,
        admission_fixture::{admitted_batch, submitted},
        batch::{
            admitted_batch::{AdmittedBatch, SubmittedEntity, prepared_chunk},
            prepare::PreparedEntity,
        },
        error::EntityError,
        notification_recording_harness::{budgeted_recording_service, geometry_budget, recording_service},
        write_kind::BatchCreate,
    };
    use andromeda_json_literal::JsonLiteral;
    use andromeda_jsonld::{
        batch_contexts::{AdmittedBatchContext, BatchContexts},
        context_spec::ContextSpec,
    };
    use andromeda_ngsi_ld_entity::document::EntityDocument;
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
    use andromeda_ngsi_ld_model::{attribute::instance::AttributeValue, entity::Entity};
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

    /// The expanded attribute names of each entity the batch still holds, by
    /// identifier, in submission order.
    fn forwardable_iris(admitted: &AdmittedBatch<BatchCreate>) -> Vec<(String, Vec<String>)> {
        admitted
            .forwardable()
            .map(|entity| (entity.id.to_string(), attribute_iris(entity.document)))
            .collect()
    }

    /// The narrowing a batch goes through once its forwarding phase answered,
    /// by the expanded names the forwarding phase reports: V1 loses `speed`, V2
    /// loses every attribute, V3 is claimed by nothing, and V4 is claimed only
    /// for an attribute it does not carry.
    fn proxied(id: &Identifier) -> Vec<AttributeName> {
        match id.as_str() {
            "urn:ngsi-ld:Vehicle:1" => names(&[SPEED_IRI]),
            "urn:ngsi-ld:Vehicle:2" => names(&[SPEED_IRI, BRAND_NAME_IRI]),
            "urn:ngsi-ld:Vehicle:4" => names(&["https://uri.etsi.org/ngsi-ld/default-context/colour"]),
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
    /// input data". The entity whose attribute a proxied registration took
    /// loses it, the entity left with none leaves the batch, and the entities
    /// the strip did not reach stay as admitted.
    #[tokio::test]
    async fn the_local_phase_keeps_what_the_proxied_registrations_left() {
        let (_service, admitted) = narrowed_batch().await;
        pretty_assertions::assert_eq!(
            forwardable_iris(&admitted),
            vec![
                ("urn:ngsi-ld:Vehicle:1".to_owned(), vec![BRAND_NAME_IRI.to_owned()]),
                ("urn:ngsi-ld:Vehicle:3".to_owned(), vec![BRAND_NAME_IRI.to_owned(), SPEED_IRI.to_owned()]),
                ("urn:ngsi-ld:Vehicle:4".to_owned(), vec![BRAND_NAME_IRI.to_owned(), SPEED_IRI.to_owned()]),
            ]
        );
    }

    /// The local phase writes the chunk in submission order, each entity from
    /// the document the admission prepared, without being prepared again.
    #[tokio::test]
    async fn a_chunk_is_prepared_in_submission_order_from_the_admitted_documents() {
        let (_service, admitted) = narrowed_batch().await;
        let (_, entities) = admitted.into_parts();
        pretty_assertions::assert_eq!(
            prepared_iris(&prepared_chunk(entities)),
            vec![
                ("urn:ngsi-ld:Vehicle:1".to_owned(), vec![BRAND_NAME_IRI.to_owned()]),
                ("urn:ngsi-ld:Vehicle:3".to_owned(), vec![BRAND_NAME_IRI.to_owned(), SPEED_IRI.to_owned()]),
                ("urn:ngsi-ld:Vehicle:4".to_owned(), vec![BRAND_NAME_IRI.to_owned(), SPEED_IRI.to_owned()]),
            ]
        );
    }

    /// The forwarding phase reads the document the admission expanded, the
    /// entity untouched by any strip included.
    #[tokio::test]
    async fn the_forwarding_phase_reads_the_admitted_documents() {
        let (service, _recorded) = recording_service().await;
        let admitted = admitted_batch(&service, BatchCreate, vec![vehicle("urn:ngsi-ld:Vehicle:1")], &ContextSpec::core_only()).await;
        pretty_assertions::assert_eq!(
            forwardable_iris(&admitted),
            vec![("urn:ngsi-ld:Vehicle:1".to_owned(), vec![BRAND_NAME_IRI.to_owned(), SPEED_IRI.to_owned()])]
        );
    }

    /// A context mapping `term` to `iri`, as an item of an
    /// `application/ld+json` batch carries it, interned in `contexts`.
    /// The context defining `term` as `iri`, admitted as a request's is.
    async fn term_context(service: &EntityService, contexts: &mut BatchContexts, term: &str, iri: &str) -> AdmittedBatchContext {
        contexts
            .context_of(Some(JsonLiteral::admit(&json!({ term: iri }).to_string()).expect("an admissible literal")))
            .admit_for_request(&service.engine)
            .await
            .expect("the context is admitted")
    }

    fn submitted_under(id: &str, context: &AdmittedBatchContext) -> SubmittedEntity<AttributeValue> {
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
        let first = term_context(&service, &mut contexts, "speed", "https://a.example/speed").await;
        let second = term_context(&service, &mut contexts, "speed", "https://b.example/speed").await;
        let submitted = vec![
            submitted_under("urn:ngsi-ld:Vehicle:1", &first),
            submitted_under("urn:ngsi-ld:Vehicle:2", &second),
            submitted_under("urn:ngsi-ld:Vehicle:3", &first),
        ];

        let (admitted, refused) = service.admit_batch_write(BatchCreate, submitted).await;

        assert!(refused.is_empty(), "{refused:?}");
        pretty_assertions::assert_eq!(
            forwardable_iris(&admitted),
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

    /// A narrowed entity keeps the names it was expanded under its own
    /// context, not under another entity's, and each entity stays tied to the
    /// context it was submitted under for the forwarding phase.
    #[tokio::test]
    async fn a_narrowed_entity_keeps_what_its_own_context_expanded() {
        let (service, _recorded) = recording_service().await;
        let mut contexts = BatchContexts::per_entity();
        let own = term_context(&service, &mut contexts, "brandName", "https://a.example/brandName").await;
        let other = term_context(&service, &mut contexts, "brandName", "https://b.example/brandName").await;
        let submitted = vec![submitted_under("urn:ngsi-ld:Vehicle:1", &own), submitted_under("urn:ngsi-ld:Vehicle:2", &other)];
        let (mut admitted, refused) = service.admit_batch_write(BatchCreate, submitted).await;
        assert!(refused.is_empty(), "{refused:?}");
        admitted.retain_local(|id| {
            if id.as_str() == "urn:ngsi-ld:Vehicle:1" {
                names(&[SPEED_IRI])
            } else {
                Vec::new()
            }
        });
        let contexts_of: Vec<_> = admitted.forwardable().map(|entity| entity.context.id()).collect();
        assert!(contexts_of[0] != contexts_of[1], "each entity keeps the context it was submitted under");

        let (_, entities) = admitted.into_parts();
        let prepared = prepared_chunk(entities);

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
        let (admitted, refused) = service
            .admit_batch_write(BatchCreate, submitted(&service, entities, &ContextSpec::core_only()).await)
            .await;
        pretty_assertions::assert_eq!(refused.iter().map(|(id, _)| id.to_string()).collect::<Vec<_>>(), vec!["urn:ngsi-ld:Vehicle:1"]);
        pretty_assertions::assert_eq!(
            admitted.forwardable().map(|entity| entity.id.to_string()).collect::<Vec<_>>(),
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

        let (admitted, refused) = service
            .admit_batch_write(BatchCreate, submitted(&service, entities, &ContextSpec::core_only()).await)
            .await;

        let [(id, refusal)] = refused.as_slice() else {
            panic!("exactly the oversized entity is refused: {refused:?}");
        };
        pretty_assertions::assert_eq!(id.as_str(), "urn:ngsi-ld:Vehicle:1");
        assert!(
            matches!(refusal, EntityError::PayloadBudget(PayloadBudgetExceeded::GeometryPositions { .. })),
            "{refusal:?}"
        );
        pretty_assertions::assert_eq!(
            admitted.forwardable().map(|entity| entity.id.to_string()).collect::<Vec<_>>(),
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

        let (admitted, refused) = service
            .admit_batch_write(BatchCreate, submitted(&service, entities, &ContextSpec::core_only()).await)
            .await;

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
            admitted.forwardable().map(|entity| entity.id.to_string()).collect::<Vec<_>>(),
            vec!["urn:ngsi-ld:Vehicle:2"]
        );
    }
}
