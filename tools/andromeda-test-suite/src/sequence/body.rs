//! Request bodies: NGSI-LD documents read and written by the `ngsi-ld` crate's typed model, and the JSON or text a
//! test sends unchecked because the model refuses it.
//!
//! A typed body is the template the test wrote, filled, read by the model of its operation and written back by the
//! model. The written form must be the document the test wrote, up to the spellings the specification reads the same
//! (an origin URL with or without its `/` path, RFC 3986 section 6.2.3; a single temporal instance or a one-element
//! array of it, ETSI GS CIM 009 v1.9.1 clauses 4.5.7 and 4.5.8; an `objectList` item as a string or as
//! `{"object": ...}`, clause 5.2.37): any other difference fails the request before it is sent, so a typed body
//! never sends something the test did not write.

use crate::sequence::{
    payload::{Payload, PayloadError},
    variables::{TemplateError, Variables},
    written_json::{read, write},
};
use ngsi_ld::{
    model::{
        Attribute,
        Change,
        Complete,
        Entity,
        EntityFragment,
        EntityId,
        Input,
        InstanceDeletion,
        LocalContext,
        Lossless,
        Patch,
        Projected,
        Representation,
        evolution::TemporalEntity,
    },
    registration::CSourceRegistration,
    snapshot::{EntityQuery, QueryDocument, Snapshot, TemporalEvolutionQuery},
    subscription::{EntitySubscription, RegistrationSubscription, Subscription},
};
use serde_json::{Map, Value as Json};

/// The `@context` member of a JSON-LD document.
const CONTEXT: &str = "@context";

/// The identifier and type a temporal attributes body is read under: the model reads a temporal entity, whose
/// identifier and type the body itself does not carry.
const WRAPPER_MEMBERS: [(&str, &str); 2] = [("id", "urn:ngsi-ld:TemporalAttributes:body"), ("type", "TemporalAttributes")];

/// The NGSI-LD document a typed body holds, which picks the model that reads and writes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::Display)]
pub enum Document {
    /// An entity (Create Entity, Replace Entity).
    Entity,
    /// An array of entities (batch create and upsert).
    Entities,
    /// An entity fragment (Append, Update, Merge Entity).
    Fragment,
    /// An array of complete entity fragments (batch update and merge).
    CompleteFragments,
    /// A partial attribute update.
    AttributePatch,
    /// An attribute (Replace Attribute, Modify Attribute Instance).
    AttributeInput,
    /// A temporal entity.
    Temporal,
    /// The attributes of a temporal entity (Add Attributes).
    TemporalAttributes,
    /// A subscription.
    Subscription,
    /// A context source registration subscription.
    RegistrationSubscription,
    /// A context source registration.
    Registration,
    /// A snapshot.
    Snapshot,
    /// An entity query (batch query, entity map).
    EntityQuery,
    /// A temporal evolution query.
    TemporalQuery,
    /// A local `@context` (Add @context).
    Context,
    /// An array of entity identifiers (batch delete).
    EntityIds,
}

/// A request body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Body {
    /// No body.
    Empty,
    /// A document the typed model reads and writes; the text is a template.
    Typed(Document, String),
    /// JSON sent as written, which the typed model refuses; the text is a template.
    Json(String),
    /// Text sent as written, without `Content-Type`; the text is a template.
    Text(String),
    /// A payload built when the request is sent, without `Content-Type`.
    Generated(Payload),
}

/// Why a body cannot be sent.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BodyError {
    /// The template cannot be filled.
    #[error(transparent)]
    Template(#[from] TemplateError),
    /// The generated payload cannot be built.
    #[error(transparent)]
    Payload(#[from] PayloadError),
    /// The filled template is not JSON.
    #[error("the {0} body is not JSON: {1}")]
    NotJson(Document, String),
    /// The typed model refuses the document.
    #[error("the {0} model refuses the body: {1}")]
    Refused(Document, String),
    /// The typed model writes something other than what the test wrote.
    #[error("the {document} model writes {written} for {body}")]
    Differs {
        /// The document.
        document: Document,
        /// What the test wrote.
        body: String,
        /// What the model writes.
        written: String,
    },
}

impl Body {
    /// The bytes to send, and whether they are JSON.
    ///
    /// # Errors
    ///
    /// Returns [`BodyError`] when the template cannot be filled or a typed body is refused or changed by the model.
    pub fn bytes(&self, variables: &Variables) -> Result<Option<(String, BodyKind)>, BodyError> {
        match self {
            Body::Empty => Ok(None),
            Body::Typed(document, template) => Ok(Some((typed(*document, &variables.fill(template)?)?, BodyKind::Json))),
            Body::Json(template) => Ok(Some((variables.fill(template)?, BodyKind::Json))),
            Body::Text(template) => Ok(Some((variables.fill(template)?, BodyKind::Text))),
            Body::Generated(payload) => Ok(Some((payload.render(variables)?, BodyKind::Text))),
        }
    }
}

/// What a body's bytes are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyKind {
    /// JSON: sent with `Content-Type: application/json` unless the request names another.
    Json,
    /// Text: sent without `Content-Type` unless the request names one.
    Text,
}

/// The document `body` holds, as the typed model of `document` writes it.
fn typed(document: Document, body: &str) -> Result<String, BodyError> {
    let written = read(body).map_err(|error| BodyError::NotJson(document, error))?;
    match document {
        Document::Entities => array(Document::Entity, &written),
        Document::CompleteFragments => array(Document::CompleteFragments, &written),
        Document::EntityIds => ids(&written).map(|()| body.to_owned()),
        Document::TemporalAttributes => temporal_attributes(&written),
        Document::Entity
        | Document::Fragment
        | Document::AttributePatch
        | Document::AttributeInput
        | Document::Temporal
        | Document::Subscription
        | Document::RegistrationSubscription
        | Document::Registration
        | Document::Snapshot
        | Document::EntityQuery
        | Document::TemporalQuery
        | Document::Context => single(document, &written),
    }
}

/// One document, written by its model in the form the test wrote it.
fn single(document: Document, written: &Json) -> Result<String, BodyError> {
    let (context, rest) = match (delivers_context(document), written) {
        (true, Json::Object(members)) if members.contains_key(CONTEXT) => {
            let mut rest = members.clone();
            let context = rest.remove(CONTEXT);
            (context, Json::Object(rest))
        }
        _ => (None, written.clone()),
    };
    let text = write(&rest);
    let renderings = renderings(document, &text).map_err(|error| BodyError::Refused(document, error))?;
    let chosen = renderings
        .iter()
        .find(|rendering| reads_as(document, rendering, &rest))
        .ok_or_else(|| BodyError::Differs {
            document,
            body: text.clone(),
            written: renderings.first().cloned().unwrap_or_default(),
        })?;
    Ok(match context {
        Some(context) => with_context(chosen, &context),
        None => chosen.clone(),
    })
}

/// Whether the document's `@context` is delivered by the client rather than read by the model.
const fn delivers_context(document: Document) -> bool {
    matches!(
        document,
        Document::Subscription
            | Document::RegistrationSubscription
            | Document::Registration
            | Document::Snapshot
            | Document::EntityQuery
            | Document::TemporalQuery
    )
}

/// `rendered` with the member `@context` set to `context` first.
fn with_context(rendered: &str, context: &Json) -> String {
    let rest = rendered.trim_start().strip_prefix('{').unwrap_or(rendered).trim_start();
    if rest.starts_with('}') {
        format!("{{\"{CONTEXT}\":{}{rest}", write(context))
    } else {
        format!("{{\"{CONTEXT}\":{},{rest}", write(context))
    }
}

/// An array of documents, each written by the model of `document`.
fn array(document: Document, written: &Json) -> Result<String, BodyError> {
    let Json::Array(items) = written else {
        return Err(BodyError::Refused(document, "the body is not an array".to_owned()));
    };
    let rendered = items.iter().map(|item| single(document, item)).collect::<Result<Vec<_>, _>>()?;
    Ok(format!("[{}]", rendered.join(",")))
}

/// Checks that every item is an entity identifier.
fn ids(written: &Json) -> Result<(), BodyError> {
    let refused = |reason: String| BodyError::Refused(Document::EntityIds, reason);
    let Json::Array(items) = written else {
        return Err(refused("the body is not an array".to_owned()));
    };
    items.iter().try_for_each(|item| match item {
        Json::String(id) => id.parse::<EntityId>().map(|_| ()).map_err(|error| refused(format!("{id}: {error}"))),
        other @ (Json::Null | Json::Bool(_) | Json::Number(_) | Json::Array(_) | Json::Object(_)) => Err(refused(format!("{other} is not an identifier"))),
    })
}

/// The attributes of a temporal entity, read and written as a temporal entity under a wrapper identifier and type.
fn temporal_attributes(written: &Json) -> Result<String, BodyError> {
    let document = Document::TemporalAttributes;
    let Json::Object(members) = written else {
        return Err(BodyError::Refused(document, "the body is not an object".to_owned()));
    };
    let mut wrapped = Map::new();
    for (name, value) in WRAPPER_MEMBERS {
        wrapped.insert(name.to_owned(), Json::String(value.to_owned()));
    }
    wrapped.extend(members.iter().map(|(name, value)| (name.clone(), value.clone())));
    let text = write(&Json::Object(wrapped));
    let renderings = renderings(Document::Temporal, &text).map_err(|error| BodyError::Refused(document, error))?;
    let unwrapped = renderings
        .iter()
        .filter_map(|rendering| match read(rendering) {
            Ok(Json::Object(mut rendered)) => {
                for (name, _) in WRAPPER_MEMBERS {
                    rendered.remove(name);
                }
                Some(Json::Object(rendered))
            }
            Ok(_) | Err(_) => None,
        })
        .collect::<Vec<_>>();
    unwrapped
        .iter()
        .find(|rendered| equivalent(document, rendered, written))
        .map(write)
        .ok_or_else(|| BodyError::Differs {
            document,
            body: write(written),
            written: unwrapped.first().map(write).unwrap_or_default(),
        })
}

/// Reads `text` in the normalized form, then in the concise form.
fn forms<E: ToString>(parse: impl Fn(Lossless) -> Result<Vec<String>, E>) -> Result<Vec<String>, String> {
    parse(Lossless::Normalized)
        .or_else(|normalized| parse(Lossless::Concise).map_err(|concise| format!("normalized: {} | concise: {}", normalized.to_string(), concise.to_string())))
}

/// Every form the model of `document` writes the document `text` in.
fn renderings(document: Document, text: &str) -> Result<Vec<String>, String> {
    let error = |error: &dyn ToString| error.to_string();
    match document {
        Document::Entity | Document::Entities => forms(|form| {
            Entity::<Input>::parse(text, form).map(|entity| {
                vec![
                    entity.render(Representation::Normalized).to_string(),
                    entity.render(Representation::Concise).to_string(),
                ]
            })
        }),
        Document::Fragment => forms(|form| {
            EntityFragment::<Projected>::parse(text, form).map(|fragment| {
                vec![
                    fragment.render(Lossless::Normalized).to_string(),
                    fragment.render(Lossless::Concise).to_string(),
                ]
            })
        }),
        Document::CompleteFragments => forms(|form| {
            EntityFragment::<Complete>::parse(text, form).map(|fragment| {
                vec![
                    fragment.render(Lossless::Normalized).to_string(),
                    fragment.render(Lossless::Concise).to_string(),
                ]
            })
        }),
        Document::AttributePatch => forms(|form| {
            Change::<Attribute<Patch>, InstanceDeletion>::parse(text, form)
                .map(|change| vec![change.render(Lossless::Normalized).to_string(), change.render(Lossless::Concise).to_string()])
        }),
        Document::AttributeInput => forms(|form| {
            Attribute::<Input>::parse_input(text, form).map(|attribute| {
                vec![
                    attribute.render(Lossless::Normalized).to_string(),
                    attribute.render(Lossless::Concise).to_string(),
                ]
            })
        }),
        Document::Temporal | Document::TemporalAttributes => forms(|form| {
            TemporalEntity::<Input>::parse(text, form).map(|temporal| {
                vec![
                    temporal.render(Lossless::Normalized).to_string(),
                    temporal.render(Lossless::Concise).to_string(),
                ]
            })
        }),
        Document::Subscription => Subscription::<Input, EntitySubscription>::parse(text)
            .map(|subscription| vec![subscription.render().to_string()])
            .map_err(|e| error(&e)),
        Document::RegistrationSubscription => Subscription::<Input, RegistrationSubscription>::parse(text)
            .map(|subscription| vec![subscription.render().to_string()])
            .map_err(|e| error(&e)),
        Document::Registration => CSourceRegistration::<Input>::parse(text)
            .map(|registration| vec![registration.render().to_string()])
            .map_err(|e| error(&e)),
        Document::Snapshot => Snapshot::<Input>::parse(text)
            .map(|snapshot| vec![snapshot.render().to_string()])
            .map_err(|e| error(&e)),
        Document::EntityQuery => QueryDocument::<EntityQuery>::parse(text)
            .map(|query| vec![query.render().to_string()])
            .map_err(|e| error(&e)),
        Document::TemporalQuery => QueryDocument::<TemporalEvolutionQuery>::parse(text)
            .map(|query| vec![query.render().to_string()])
            .map_err(|e| error(&e)),
        Document::Context => LocalContext::parse(text)
            .map(|context| vec![context.render().to_string()])
            .map_err(|e| error(&e)),
        Document::EntityIds => Err("an array of identifiers has no model".to_owned()),
    }
}

/// Whether the JSON text `rendering` is the document `written`, up to the spellings read the same.
fn reads_as(document: Document, rendering: &str, written: &Json) -> bool {
    read(rendering).is_ok_and(|rendered| equivalent(document, &rendered, written))
}

/// Whether `rendered` is `written`, up to the spellings read the same.
fn equivalent(document: Document, rendered: &Json, written: &Json) -> bool {
    let temporal = matches!(document, Document::Temporal | Document::TemporalAttributes);
    rendered == written || *rendered == canonical(written, if temporal { Depth::Top } else { Depth::Nested })
}

/// Whether a value is the top of a temporal document, whose attributes may hold a single instance, or anything else.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Depth {
    /// The temporal document itself.
    Top,
    /// A member or item of it, or a document that is not temporal.
    Nested,
}

/// `written` with each spelling the crate writes differently replaced by the crate's: an origin URL gains its `/`
/// path, an `objectList` string item becomes `{"object": ...}`, and an attribute of a temporal document holding one
/// instance becomes a one-element array.
fn canonical(written: &Json, depth: Depth) -> Json {
    match written {
        Json::String(text) if is_origin(text) => Json::String(format!("{text}/")),
        Json::Array(items) => Json::Array(items.iter().map(|item| canonical(item, Depth::Nested)).collect()),
        Json::Object(members) => Json::Object(
            members
                .iter()
                .map(|(name, value)| {
                    let value = match (name.as_str(), value) {
                        ("objectList", Json::Array(items)) => Json::Array(
                            items
                                .iter()
                                .map(|item| match item {
                                    Json::String(object) => {
                                        let mut wrapped = Map::new();
                                        wrapped.insert("object".to_owned(), Json::String(object.clone()));
                                        Json::Object(wrapped)
                                    }
                                    other @ (Json::Null | Json::Bool(_) | Json::Number(_) | Json::Array(_) | Json::Object(_)) => other.clone(),
                                })
                                .collect(),
                        ),
                        ("id" | "type" | CONTEXT | "scope", other) => other.clone(),
                        (_, Json::Object(instance)) if depth == Depth::Top && is_instance(instance) => Json::Array(vec![Json::Object(instance.clone())]),
                        (_, other) => other.clone(),
                    };
                    (name.clone(), canonical(&value, Depth::Nested))
                })
                .collect(),
        ),
        other @ (Json::Null | Json::Bool(_) | Json::Number(_) | Json::String(_)) => other.clone(),
    }
}

/// Whether a member of a temporal document is a single attribute instance.
fn is_instance(members: &Map<String, Json>) -> bool {
    members.get("type").and_then(Json::as_str).is_some_and(|kind| {
        matches!(
            kind,
            "Property" | "Relationship" | "GeoProperty" | "LanguageProperty" | "VocabProperty" | "JsonProperty" | "ListProperty" | "ListRelationship"
        )
    })
}

/// Whether `text` is an origin: a scheme and an authority with no path.
fn is_origin(text: &str) -> bool {
    ["http://", "https://"].iter().any(|scheme| {
        text.strip_prefix(scheme)
            .is_some_and(|authority| !authority.is_empty() && !authority.contains(['/', '?', '#']))
    })
}

#[cfg(test)]
mod tests {
    use crate::sequence::{
        body::{Body, BodyError, BodyKind, Document},
        variables::Variables,
    };
    use googletest::prelude::*;

    fn variables() -> Variables {
        let mut variables = Variables::default();
        variables.set_text("ns", "t1");
        variables
    }

    #[gtest]
    fn a_typed_entity_is_sent_as_the_model_writes_it() -> Result<()> {
        let body = Body::Typed(
            Document::Entity,
            r#"{"id": "urn:ngsi-ld:Building:{{ns}}-1", "type": "Building", "name": {"type": "Property", "value": "x"}}"#.to_owned(),
        );
        let (bytes, kind) = body.bytes(&variables())?.or_fail()?;
        expect_that!(kind, eq(BodyKind::Json));
        let sent: serde_json::Value = serde_json::from_str(&bytes)?;
        verify_that!(sent["id"], eq(&serde_json::json!("urn:ngsi-ld:Building:t1-1")))
    }

    #[gtest]
    fn a_body_the_model_refuses_is_an_error() {
        let body = Body::Typed(Document::Entity, r#"{"type": "Building"}"#.to_owned());
        expect_that!(matches!(body.bytes(&variables()), Err(BodyError::Refused(_, _))), eq(true));
    }

    #[gtest]
    fn a_subscription_keeps_the_context_the_test_wrote() -> Result<()> {
        let body = Body::Typed(
            Document::Subscription,
            r#"{"@context": "https://example.org/c.jsonld", "id": "urn:ngsi-ld:Subscription:{{ns}}-1", "type": "Subscription", "entities": [{"type": "Building"}], "notification": {"endpoint": {"uri": "http://example.org/notify"}}}"#
                .to_owned(),
        );
        let (bytes, _) = body.bytes(&variables())?.or_fail()?;
        let sent: serde_json::Value = serde_json::from_str(&bytes)?;
        verify_that!(sent["@context"], eq(&serde_json::json!("https://example.org/c.jsonld")))
    }

    #[gtest]
    fn unchecked_json_is_sent_as_written() -> Result<()> {
        let body = Body::Json(r#"{"id": 1}"#.to_owned());
        verify_that!(body.bytes(&variables())?, some(eq(&(r#"{"id": 1}"#.to_owned(), BodyKind::Json))))
    }
}
