//! User contexts (clause 5.5.7): what a request's `@context` decides, what the
//! core context keeps for itself, and what the walk leaves to the processor.

use crate::{
    decline::Decline,
    entity_expansion::EntityExpansion,
    expansion_fixture::{TERMS, UNSUPPORTED, VOCAB_AND_LANGUAGE, admitted, engine, entity, patch},
    normalization::Normalization,
    typed_expander::TypedExpander,
    typed_expansion::TypedExpansion,
};
use andromeda_json_literal::piece::JsonPiece;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_entity::{
    document::{
        EntityDocument,
        instance_document::InstanceDocument,
        written_value::{ValueSlot, WrittenValue},
    },
    expanded_value::{ExpandedValue, ValueNode},
    literal::LiteralValue,
    value_mode::{Fragment, FragmentMember},
};
use andromeda_ngsi_ld_model::{
    attribute::{instance::AttributeValue, member_reading::MemberReading, merge_member::MergeMember},
    entity::EntityPatch,
};
use iri_rs::IriBuf;
use serde_json::{Value, json, value::RawValue};

/// The one instance written for `attribute`.
fn instance<'a>(document: &'a EntityDocument, attribute: &str) -> &'a InstanceDocument {
    let member = document
        .attributes()
        .get(attribute)
        .unwrap_or_else(|| panic!("{attribute} is written, got {document:?}"));
    &member.instances()[0]
}

/// The structured value an instance carries.
fn node(instance: &InstanceDocument) -> &ValueNode<Fragment> {
    let ValueSlot::Set(WrittenValue::Value(values)) = instance.value() else {
        panic!("a Property value");
    };
    let [ExpandedValue::Node(node)] = values.as_slice() else {
        panic!("one node");
    };
    node
}

/// The values of one member of a structured value.
fn member<'a>(node: &'a ValueNode<Fragment>, name: &str) -> &'a [ExpandedValue<Fragment>] {
    let Some(FragmentMember::Values(values)) = node.members().get(name) else {
        panic!("{name} is a member of {node:?}");
    };
    values
}

/// Every coercion a term definition can make, decided by the request's
/// `@context`: a prefix, `@id`, `@vocab`, a datatype, `@json`, a `@language`
/// container, a `@list` container and a term language.
#[tokio::test]
async fn the_request_context_decides_every_term_it_defines() {
    let document = entity(
        &engine(),
        json!({
            "@context": TERMS,
            "id": "urn:ngsi-ld:Car:1", "type": "Car",
            "speed": {"type": "Property", "value": {
                "sensor": "urn:ngsi-ld:Sensor:1",
                "category": "Car",
                "label": {"en": "red", "@none": "rouge"},
                "settings": {"k": [1, 2.50]},
                "measuredAt": "2024-01-01T00:00:00Z",
                "tags": ["a", "b"],
                "nickname": "Titi",
                "ex:raw": true
            }}
        }),
        Normalization::Entity,
    )
    .await
    .expect("the fixture expands")
    .agreed();

    pretty_assertions::assert_eq!(document.types()[0].as_str(), "https://example.org/terms#Car");
    let value = node(instance(&document, "https://example.org/terms#speed"));
    let [ExpandedValue::Node(sensor)] = member(value, "https://example.org/terms#sensor") else {
        panic!("an @id-typed term makes a node reference");
    };
    assert!(sensor.is_reference());
    let [ExpandedValue::Node(category)] = member(value, "https://example.org/terms#category") else {
        panic!("a @vocab-typed term makes a node reference");
    };
    pretty_assertions::assert_eq!(category.id().map(ToString::to_string).as_deref(), Some("https://example.org/terms#Car"));
    pretty_assertions::assert_eq!(member(value, "https://example.org/terms#label").len(), 2);
    assert!(matches!(member(value, "https://example.org/terms#settings"), [ExpandedValue::Json(_)]));
    let [ExpandedValue::Literal(measured)] = member(value, "https://example.org/terms#measuredAt") else {
        panic!("a typed literal");
    };
    pretty_assertions::assert_eq!(
        measured.datatype().map(ToString::to_string).as_deref(),
        Some("https://uri.etsi.org/ngsi-ld/DateTime")
    );
    assert!(matches!(member(value, "https://example.org/terms#tags"), [ExpandedValue::List(items)] if items.len() == 2));
    let [ExpandedValue::Literal(nickname)] = member(value, "https://example.org/terms#nickname") else {
        panic!("a language-tagged literal");
    };
    pretty_assertions::assert_eq!(nickname.language().map(ToString::to_string).as_deref(), Some("fr"));
    let [ExpandedValue::Literal(raw)] = member(value, "https://example.org/terms#raw") else {
        panic!("a compact IRI key");
    };
    pretty_assertions::assert_eq!(raw.value(), &LiteralValue::Boolean(true));
}

/// Clause 5.5.7: the core context is processed last and protected, so a user
/// context redefining `location`, `value` or `observedAt` leaves the core
/// definitions in force.
#[tokio::test]
async fn a_user_context_cannot_redefine_a_core_term() {
    let document = entity(
        &engine(),
        json!({
            "@context": TERMS,
            "id": "urn:ngsi-ld:Car:2", "type": "Car",
            "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [1.0, 2.0]}, "observedAt": "2024-01-01T00:00:00Z"}
        }),
        Normalization::Entity,
    )
    .await
    .expect("the fixture expands")
    .agreed();

    let location = instance(&document, "https://uri.etsi.org/ngsi-ld/location");
    assert!(matches!(location.value(), ValueSlot::Set(WrittenValue::Geometry(_))));
    assert!(location.observed_at().is_some());
}

/// The core `@vocab` is processed last, so a user `@vocab` never applies,
/// while a user default `@language` tags every plain string.
#[tokio::test]
async fn the_core_vocabulary_shadows_a_user_one_and_a_default_language_tags_strings() {
    let document = patch(
        &engine(),
        json!({"@context": VOCAB_AND_LANGUAGE, "note": {"type": "Property", "value": "hello"}, "headline": {"type": "Property", "value": "t"}}),
        Normalization::Entity,
    )
    .await
    .expect("the fixture expands")
    .agreed();

    let note = instance(&document, "https://uri.etsi.org/ngsi-ld/default-context/note");
    let ValueSlot::Set(WrittenValue::Value(values)) = note.value() else {
        panic!("a value");
    };
    let [ExpandedValue::Literal(text)] = values.as_slice() else {
        panic!("a literal");
    };
    pretty_assertions::assert_eq!(text.language().map(ToString::to_string).as_deref(), Some("en"));
    assert!(document.attributes().contains_key("https://example.org/vocab#headline"));
}

/// A context named in a `Link` header decides terms as a body context does.
#[tokio::test]
async fn a_linked_context_decides_terms_like_a_body_context() {
    let engine = engine();
    let spec = ContextSpec::from_link(IriBuf::new(TERMS.to_owned()).expect("an IRI"));
    let context = engine.expansion_context(&spec).await.expect("the context processes");
    let (body, _) = admitted(json!({"speed": {"type": "Property", "value": 1}}));
    let fragment: EntityPatch = serde_json::from_value(body).expect("a typed fragment");
    let TypedExpansion::Expanded(document) = TypedExpander::new(&context).entity_patch(&fragment, Normalization::Entity).expect("expands") else {
        panic!("the walk expands it");
    };
    assert!(document.attributes().contains_key("https://example.org/terms#speed"));
}

/// Clause 4.5.2.2: `ngsildproof` carries its two seals under the property-scoped
/// context the core context gives the term. That scoped context names the W3C
/// Data Integrity context, whose `DataIntegrityProof` declares a type-scoped
/// context of its own, so the walk leaves a proof to the processor, which
/// expands its members to their W3C IRIs.
#[tokio::test]
async fn the_proof_term_expands_under_its_scoped_context() {
    let both = entity(
        &engine(),
        json!({
            "id": "urn:ngsi-ld:Store:1", "type": "Store",
            "ngsildproof": {
                "type": "Property",
                "value": {"type": "DataIntegrityProof", "cryptosuite": "eddsa-jcs-2022"},
                "entityIdSealed": "urn:ngsi-ld:Store:1",
                "entityTypeSealed": "Store"
            }
        }),
        Normalization::Entity,
    )
    .await
    .expect("the fixture expands");
    pretty_assertions::assert_eq!(both.typed.expect("no refusal"), TypedExpansion::Declined(Decline::TypeScopedContext));
    let document = both.processor.expect("the processor expands a proof");

    let proof = instance(&document, "https://uri.etsi.org/ngsi-ld/ngsildproof");
    assert!(proof.proof_seals().entity_id().is_some());
    assert!(proof.proof_seals().entity_type().is_some());
    let value = node(proof);
    assert!(value.types().iter().any(|name| name.as_str() == "https://w3id.org/security#DataIntegrityProof"));
    pretty_assertions::assert_eq!(member(value, "https://w3id.org/security#cryptosuite").len(), 1);
}

/// What the walk does not model it declines, and the processor expands
/// instead: the expansion with the fallback answers exactly what the processor
/// reads, a document or the refusal of a shape the model cannot hold (a
/// reverse property).
#[tokio::test]
async fn what_the_walk_does_not_model_goes_to_the_processor() {
    let engine = engine();
    let cases: [(Value, Decline); 4] = [
        (
            json!({"@context": UNSUPPORTED, "a": {"type": "Property", "value": {"byKey": {"k": 1}}}}),
            Decline::UnsupportedContainer,
        ),
        (
            json!({"@context": UNSUPPORTED, "a": {"type": "Property", "value": {"partOf": {"@id": "urn:x:1"}}}}),
            Decline::ReverseProperty,
        ),
        (
            json!({"@context": UNSUPPORTED, "a": {"type": "Property", "value": {"type": "Typed", "inner": 1}}}),
            Decline::TypeScopedContext,
        ),
        (
            json!({"a": {"type": "Property", "value": {"@context": {"x": "urn:x"}, "x": 1}}}),
            Decline::EmbeddedContext,
        ),
    ];
    for (body, expected) in cases {
        let both = patch(&engine, body.clone(), Normalization::Entity).await.expect("the fixture expands");
        pretty_assertions::assert_eq!(both.typed.expect("no refusal"), TypedExpansion::Declined(expected));
        let (body, spec) = admitted(body);
        let fragment: EntityPatch = serde_json::from_value(body).expect("a typed fragment");
        let expansion = EntityExpansion::prepare(&engine, &spec).await.expect("the context processes");
        let processed = expansion
            .entity_patch(&fragment, Normalization::Entity)
            .await
            .map_err(|error| error.to_string());
        pretty_assertions::assert_eq!(processed, both.processor.map_err(|refusal| refusal.to_string()));
    }
}

/// The expansion hands an attribute fragment the walk declines to the
/// processor and reads its answer: an embedded `@context` the processor
/// processes on top of the request's (clause 5.5.7 refuses one in a client
/// request before expansion; a document a context source sent may carry it).
#[tokio::test]
async fn an_attribute_fragment_the_walk_declines_is_expanded_by_the_processor() {
    let engine = engine();
    let expansion = EntityExpansion::prepare(&engine, &ContextSpec::none())
        .await
        .expect("the core context processes");
    let name = "reading".parse().expect("an attribute name");
    let written = json!({"type": "Property", "value": {"@context": {"level": "https://example.org/level"}, "level": 3}});
    let text = RawValue::from_string(written.to_string()).expect("the fixture is JSON");
    let written_member = AttributeValue::from_fragment_node("reading", JsonPiece::new(&text)).expect("a typed attribute");
    let fragment = expansion.attribute(&name, &written_member).await.expect("the processor expands it");

    pretty_assertions::assert_eq!(fragment.name().as_str(), "https://uri.etsi.org/ngsi-ld/default-context/reading");
    let value = node(fragment.instance());
    pretty_assertions::assert_eq!(member(value, "https://example.org/level").len(), 1);
}

/// The merge fragment `body` expands to under its own `@context`.
async fn merge_document(body: Value) -> EntityDocument {
    let engine = engine();
    let (body, spec) = admitted(body);
    let context = engine.expansion_context(&spec).await.expect("the context processes");
    let fragment: EntityPatch<MergeMember> = serde_json::from_value(body).expect("a typed merge fragment");
    let TypedExpansion::Expanded(document) = TypedExpander::new(&context)
        .entity_patch(&fragment, Normalization::MemberDeletions)
        .expect("expands")
    else {
        panic!("the walk expands it");
    };
    document
}

/// The datatype of the one literal the instance of `attribute` holds.
fn literal_datatype(document: &EntityDocument, attribute: &str) -> Option<String> {
    let ValueSlot::Set(WrittenValue::Value(values)) = instance(document, attribute).value() else {
        panic!("a Property value");
    };
    let [ExpandedValue::Literal(literal)] = values.as_slice() else {
        panic!("one literal, got {values:?}");
    };
    literal.datatype().map(ToString::to_string)
}

/// JSON-LD 1.1 value expansion reads a bare scalar under a coerced term as a
/// typed literal, so a concise Property under a term coerced to `DateTime` is a
/// declared time value; the coercion does not reach the `value` member of a
/// normalized one (annex C.6).
#[tokio::test]
async fn a_concise_merge_value_under_a_term_coerced_to_a_time_value_form_is_that_typed_literal() {
    let coercing = json!({"testedAt": {"@id": "https://example.org/testedAt", "@type": "https://uri.etsi.org/ngsi-ld/DateTime"}});
    let concise = merge_document(json!({"testedAt": "2018-12-04T12:00:00Z", "@context": [coercing]})).await;
    pretty_assertions::assert_eq!(
        literal_datatype(&concise, "https://example.org/testedAt").as_deref(),
        Some("https://uri.etsi.org/ngsi-ld/DateTime")
    );
    let normalized = merge_document(json!({"testedAt": {"type": "Property", "value": "2018-12-04T12:00:00Z"}, "@context": [coercing]})).await;
    pretty_assertions::assert_eq!(literal_datatype(&normalized, "https://example.org/testedAt"), None);
}
