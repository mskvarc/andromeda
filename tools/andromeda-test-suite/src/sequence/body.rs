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
    body_rendering::{ModelRefusal, renderings},
    body_spelling::{equivalent, reads_as},
    payload::{Payload, PayloadError},
    variables::{TemplateError, Variables},
    written_json::{read, write},
};
use ngsi_ld::model::{EntityId, UriError};
use serde_json::{Map, Value as Json};

/// The `@context` member of a JSON-LD document.
pub(crate) const CONTEXT: &str = "@context";

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
#[derive(Debug, thiserror::Error)]
pub enum BodyError {
    /// The template cannot be filled.
    #[error(transparent)]
    Template(#[from] TemplateError),
    /// The generated payload cannot be built.
    #[error(transparent)]
    Payload(#[from] PayloadError),
    /// The filled template is not JSON.
    #[error("the {0} body is not JSON: {1}")]
    NotJson(Document, #[source] serde_json::Error),
    /// The typed model refuses the document.
    #[error("the {0} model refuses the body: {1}")]
    Refused(Document, #[source] ModelRefusal),
    /// The body is not the JSON shape the document has.
    #[error("the {document} body is not {expected}")]
    Shape {
        /// The document.
        document: Document,
        /// The shape it has.
        expected: JsonShape,
    },
    /// An item of an array of entity identifiers is not a URI.
    #[error("an identifier of the {0} body is not a URI")]
    Identifier(Document, #[source] UriError),
    /// An item of an array of entity identifiers is not a string.
    #[error("{} is not an identifier", write(.0))]
    NotAnIdentifier(Json),
    /// The typed model writes something other than what the test wrote.
    #[error("the {document} model writes {} for {}", .written.as_deref().map(write).unwrap_or_default(), write(.body))]
    Differs {
        /// The document.
        document: Document,
        /// What the test wrote.
        body: Box<Json>,
        /// The first form the model writes it in, absent when the model writes none the suite reads.
        written: Option<Box<Json>>,
    },
}

/// The JSON shape a body has.
#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::Display)]
pub enum JsonShape {
    /// A JSON array.
    #[strum(serialize = "an array")]
    Array,
    /// A JSON object.
    #[strum(serialize = "an object")]
    Object,
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
    let renderings = renderings(document, &text).map_err(|refusal| BodyError::Refused(document, refusal))?;
    let chosen = renderings
        .iter()
        .find(|rendering| reads_as(document, rendering, &rest))
        .ok_or_else(|| BodyError::Differs {
            document,
            body: Box::new(rest.clone()), // clone: the error owns the document the comparison read
            written: renderings.first().and_then(|first| read(first).ok()).map(Box::new),
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
        return Err(BodyError::Shape {
            document,
            expected: JsonShape::Array,
        });
    };
    let rendered = items.iter().map(|item| single(document, item)).collect::<Result<Vec<_>, _>>()?;
    Ok(format!("[{}]", rendered.join(",")))
}

/// Checks that every item is an entity identifier.
fn ids(written: &Json) -> Result<(), BodyError> {
    let Json::Array(items) = written else {
        return Err(BodyError::Shape {
            document: Document::EntityIds,
            expected: JsonShape::Array,
        });
    };
    items.iter().try_for_each(|item| match item {
        Json::String(id) => id
            .parse::<EntityId>()
            .map(|_| ())
            .map_err(|error| BodyError::Identifier(Document::EntityIds, error)),
        // clone: the error owns the item the body holds
        other @ (Json::Null | Json::Bool(_) | Json::Number(_) | Json::Array(_) | Json::Object(_)) => Err(BodyError::NotAnIdentifier(other.clone())),
    })
}

/// The attributes of a temporal entity, read and written as a temporal entity under a wrapper identifier and type.
fn temporal_attributes(written: &Json) -> Result<String, BodyError> {
    let document = Document::TemporalAttributes;
    let Json::Object(members) = written else {
        return Err(BodyError::Shape {
            document,
            expected: JsonShape::Object,
        });
    };
    let mut wrapped = Map::new();
    for (name, value) in WRAPPER_MEMBERS {
        wrapped.insert(name.to_owned(), Json::String(value.to_owned()));
    }
    wrapped.extend(members.iter().map(|(name, value)| (name.clone(), value.clone())));
    let text = write(&Json::Object(wrapped));
    let renderings = renderings(Document::Temporal, &text).map_err(|refusal| BodyError::Refused(document, refusal))?;
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
            body: Box::new(written.clone()), // clone: the error owns the document the comparison read
            written: unwrapped.first().cloned().map(Box::new),
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
