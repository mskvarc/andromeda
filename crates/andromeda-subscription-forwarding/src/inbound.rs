//! The notifications this broker receives under the identifiers it minted
//! (clauses 5.3.1, 5.3.2, 5.8.6, 5.11), decoded straight from their text.
//!
//! Two kinds arrive, told apart by their `type`: a `Notification` of a
//! forwarded copy of a local subscription, and a `ContextSourceNotification`
//! of a companion subscription this broker placed on its own registrations.
//! The envelope is read in one pass, its `data` kept as the slice of text it
//! is, and each element of `data` is then read on its own, so one element the
//! model cannot hold is refused and skipped rather than failing the
//! notification, and no JSON tree is built on the way.

use andromeda_json_literal::piece::{JsonKind, JsonPiece};
use andromeda_ngsi_ld_lexicon::{identifier::Identifier, rfc3339::RequestInstant};
use andromeda_ngsi_ld_model::{
    notification::CsrTriggerReason,
    received::{ReceivedElement, ReceivedEntities, decoded_elements},
    registration::ContextSourceRegistration,
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::value::RawValue;
use std::borrow::Cow;
use strum::Display;
use thiserror::Error;

/// The `type` of a Notification (Table 5.3.1-1).
const NOTIFICATION: &str = "Notification";
/// The `type` of a Context Source Notification (Table 5.3.2-1).
const CSOURCE_NOTIFICATION: &str = "ContextSourceNotification";
/// The `type` of a `GeoJSON` `FeatureCollection` (IETF RFC 7946).
const FEATURE_COLLECTION: &str = "FeatureCollection";

/// A notification a Context Source sent for a forwarded copy of a local
/// subscription (clause 5.3.1).
#[derive(Debug)]
pub struct InboundNotification {
    /// The sender's notification identifier.
    pub id: Identifier,
    /// The subscription the notification answers, as the sender knows it.
    pub subscription_id: Identifier,
    /// When the sender generated the notification.
    pub notified_at: DateTime<Utc>,
    /// What it carries.
    pub data: InboundData,
}

/// The `data` member of a received Notification (Table 5.3.1-1): "NGSI-LD
/// Entity[] or `FeatureCollection`".
#[derive(Debug)]
pub enum InboundData {
    /// The notified Entities, element by element, each with its own
    /// `@context` when it names one.
    Entities(ReceivedEntities),
    /// The notified Entities as one clause 5.2.30 `GeoJSON`
    /// `FeatureCollection`, which no forwarded copy asks for.
    FeatureCollection,
}

/// A notification of a companion subscription (clause 5.3.2): this broker's
/// own registration subscription notifying it.
#[derive(Debug)]
pub struct InboundCsourceNotification {
    /// The sender's notification identifier.
    pub id: Identifier,
    /// The subscription the notification answers, as the sender knows it.
    pub subscription_id: Identifier,
    /// When the sender generated the notification.
    pub notified_at: DateTime<Utc>,
    /// Why the notification fired.
    pub trigger_reason: Option<CsrTriggerReason>,
    /// The registrations it carries, element by element.
    pub data: Vec<ReceivedElement<ContextSourceRegistration>>,
}

/// The two notification kinds the receiver accepts.
#[derive(Debug)]
pub enum Inbound {
    /// A notification of a forwarded subscription copy (clause 5.3.1).
    Notification(InboundNotification),
    /// A notification of a companion registration subscription (clause 5.3.2).
    CsourceNotification(InboundCsourceNotification),
}

/// Why a document is not one of the notifications the receiver accepts.
#[derive(Debug, Error)]
pub enum InboundError {
    /// `type` is absent or not a string.
    #[error("missing `type`")]
    MissingType,
    /// `type` names neither notification kind.
    #[error("unsupported type; expected `Notification` or `ContextSourceNotification`")]
    UnsupportedType,
    /// A member the notification requires is absent.
    #[error("missing `{0}`")]
    MissingMember(EnvelopeMember),
    /// `data` is neither an Entity array nor a `FeatureCollection` (Table
    /// 5.3.1-1).
    #[error("`data` is neither an Entity array nor a FeatureCollection")]
    NotificationData,
    /// `data` is not an array of registrations (Table 5.3.2-1).
    #[error("`data` is not an array of context source registrations")]
    CsourceNotificationData,
    /// The members do not form a notification.
    #[error("{0}")]
    Shape(#[from] serde_json::Error),
}

/// A member every notification carries (Tables 5.3.1-1 and 5.3.2-1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display)]
pub enum EnvelopeMember {
    /// `id`.
    #[strum(serialize = "id")]
    Id,
    /// `subscriptionId`.
    #[strum(serialize = "subscriptionId")]
    SubscriptionId,
    /// `notifiedAt`.
    #[strum(serialize = "notifiedAt")]
    NotifiedAt,
    /// `data`.
    #[strum(serialize = "data")]
    Data,
}

/// The members of either notification, read in one pass, `data` kept as text.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Envelope<'a> {
    /// `type`.
    #[serde(rename = "type", borrow, default)]
    type_: Option<Cow<'a, str>>,
    /// `id`.
    #[serde(default)]
    id: Option<Identifier>,
    /// `subscriptionId`.
    #[serde(default)]
    subscription_id: Option<Identifier>,
    /// `notifiedAt`, read as every instant a request carries (clause 4.6.3).
    #[serde(default)]
    notified_at: Option<RequestInstant>,
    /// `data`, as written.
    #[serde(borrow, default)]
    data: Option<&'a RawValue>,
    /// `triggerReason`.
    #[serde(default)]
    trigger_reason: Option<CsrTriggerReason>,
}

impl Inbound {
    /// Decode a received notification from its text.
    ///
    /// # Errors
    ///
    /// Returns [`InboundError`] for a text that is no notification of either
    /// kind; an element of `data` that does not decode is a refused element,
    /// not an error.
    pub fn from_slice(text: &[u8]) -> Result<Inbound, InboundError> {
        let envelope: Envelope<'_> = serde_json::from_slice(text)?;
        let kind = envelope.type_.as_deref().ok_or(InboundError::MissingType)?;
        let id = envelope.id.ok_or(InboundError::MissingMember(EnvelopeMember::Id))?;
        let subscription_id = envelope.subscription_id.ok_or(InboundError::MissingMember(EnvelopeMember::SubscriptionId))?;
        let notified_at = envelope.notified_at.ok_or(InboundError::MissingMember(EnvelopeMember::NotifiedAt))?.get();
        match kind {
            NOTIFICATION => {
                let data = envelope.data.map(JsonPiece::new).ok_or(InboundError::MissingMember(EnvelopeMember::Data))?;
                Ok(Inbound::Notification(InboundNotification {
                    id,
                    subscription_id,
                    notified_at,
                    data: notification_data(data)?,
                }))
            }
            CSOURCE_NOTIFICATION => {
                let data = match envelope.data.map(JsonPiece::new) {
                    None => Vec::new(),
                    Some(data) => decoded_elements(data.elements().ok_or(InboundError::CsourceNotificationData)?),
                };
                Ok(Inbound::CsourceNotification(InboundCsourceNotification {
                    id,
                    subscription_id,
                    notified_at,
                    trigger_reason: envelope.trigger_reason,
                    data,
                }))
            }
            _unsupported => Err(InboundError::UnsupportedType),
        }
    }

    /// The subscription identifier the notification names.
    #[must_use]
    pub const fn subscription_id(&self) -> &Identifier {
        match self {
            Inbound::Notification(notification) => &notification.subscription_id,
            Inbound::CsourceNotification(notification) => &notification.subscription_id,
        }
    }
}

/// The `data` of a Notification: an Entity array, or a `FeatureCollection`.
fn notification_data(data: JsonPiece<'_>) -> Result<InboundData, InboundError> {
    match data.kind() {
        JsonKind::Array => Ok(InboundData::Entities(ReceivedEntities::from_elements(data.elements().unwrap_or_default()))),
        JsonKind::Object if data.member("type").and_then(JsonPiece::as_str).as_deref() == Some(FEATURE_COLLECTION) => Ok(InboundData::FeatureCollection),
        JsonKind::Object | JsonKind::Null | JsonKind::Boolean | JsonKind::Number | JsonKind::String => Err(InboundError::NotificationData),
    }
}

#[cfg(test)]
mod tests {
    use crate::inbound::{Inbound, InboundData, InboundError};
    use andromeda_ngsi_ld_model::{notification::CsrTriggerReason, received::ReceivedElement};

    const ENVELOPE: &str = r#""id":"urn:ngsi-ld:Notification:1","subscriptionId":"urn:ngsi-ld:Subscription:1","notifiedAt":"2024-06-15T12:30:00Z""#;
    const VEHICLE: &str = r#"{"id":"urn:ngsi-ld:Vehicle:1","type":"Vehicle","speed":{"type":"Property","value":1.10}}"#;

    fn inbound(text: &str) -> Result<Inbound, InboundError> {
        Inbound::from_slice(text.as_bytes())
    }

    #[test]
    fn a_notification_is_decoded_without_a_tree() {
        let Ok(Inbound::Notification(notification)) = inbound(&format!(r#"{{"type":"Notification",{ENVELOPE},"data":[{VEHICLE},{VEHICLE}]}}"#)) else {
            panic!("a notification");
        };
        let InboundData::Entities(entities) = notification.data else {
            panic!("an entity array");
        };
        assert!(matches!(entities.0.as_slice(), [ReceivedElement::Read(_), ReceivedElement::Read(_)]));
    }

    /// Clause 4.6.3 admits at most six fractional-second digits.
    #[test]
    fn a_notified_at_beyond_six_fractional_digits_is_refused() {
        let text = |notified_at: &str| {
            format!(
                r#"{{"type":"Notification","id":"urn:ngsi-ld:Notification:1","subscriptionId":"urn:ngsi-ld:Subscription:1","notifiedAt":"{notified_at}","data":[{VEHICLE}]}}"#
            )
        };
        assert!(inbound(&text("2024-06-15T12:30:00.123456789Z")).is_err());
        assert!(inbound(&text("2024-06-15T12:30:00.1234567Z")).is_err());
        assert!(inbound(&text("2024-06-15T12:30:00.123456Z")).is_ok());
    }

    #[test]
    fn one_bad_entity_is_refused_and_the_others_kept() {
        let Ok(Inbound::Notification(notification)) = inbound(&format!(
            r#"{{"type":"Notification",{ENVELOPE},"data":[{VEHICLE},{{"id":"urn:x"}},{VEHICLE}]}}"#
        )) else {
            panic!("a notification");
        };
        let InboundData::Entities(entities) = notification.data else {
            panic!("an entity array");
        };
        assert!(matches!(
            entities.0.as_slice(),
            [ReceivedElement::Read(_), ReceivedElement::Refused(_), ReceivedElement::Read(_)]
        ));
    }

    /// An entity carrying its own `@context`, as an `application/ld+json`
    /// notification writes it, is read with its context kept beside it.
    #[test]
    fn an_entity_context_is_kept() {
        let text =
            format!(r#"{{"type":"Notification",{ENVELOPE},"data":[{{"@context":"https://x/c.jsonld","id":"urn:ngsi-ld:Vehicle:1","type":"Vehicle"}}]}}"#);
        let Ok(Inbound::Notification(notification)) = inbound(&text) else {
            panic!("a notification");
        };
        let InboundData::Entities(entities) = notification.data else {
            panic!("an entity array");
        };
        let [ReceivedElement::Read(contextual)] = entities.0.as_slice() else {
            panic!("one entity is read");
        };
        assert!(contextual.context.is_some());
    }

    #[test]
    fn a_csource_notification_decodes_registrations_per_element() {
        let registration = r#"{"id":"urn:ngsi-ld:ContextSourceRegistration:1","type":"ContextSourceRegistration","information":[{"entities":[{"type":"Vehicle"}]}],"endpoint":"http://source.example/","createdAt":"2024-01-01T00:00:00Z","modifiedAt":"2024-01-01T00:00:00Z"}"#;
        let text = format!(r#"{{"type":"ContextSourceNotification",{ENVELOPE},"triggerReason":"newlyMatching","data":[{registration},{{}}]}}"#);
        let Ok(Inbound::CsourceNotification(notification)) = inbound(&text) else {
            panic!("a context source notification");
        };
        assert!(
            matches!(notification.data.as_slice(), [ReceivedElement::Read(_), ReceivedElement::Refused(_)]),
            "{:?}",
            notification.data
        );
        pretty_assertions::assert_eq!(notification.trigger_reason, Some(CsrTriggerReason::NewlyMatching));
    }

    #[test]
    fn a_foreign_type_is_unsupported() {
        assert!(matches!(
            inbound(&format!(r#"{{"type":"Entity",{ENVELOPE},"data":[]}}"#)),
            Err(InboundError::UnsupportedType)
        ));
    }

    #[test]
    fn a_missing_type_is_reported() {
        assert!(matches!(inbound(&format!(r#"{{{ENVELOPE},"data":[]}}"#)), Err(InboundError::MissingType)));
    }

    #[test]
    fn a_malformed_envelope_is_a_shape_error() {
        let text = r#"{"type":"Notification","id":"urn:ngsi-ld:Notification:1","subscriptionId":"urn:ngsi-ld:Subscription:1","notifiedAt":5,"data":[]}"#;
        assert!(matches!(inbound(text), Err(InboundError::Shape(_))));
    }

    #[test]
    fn a_feature_collection_is_recognised() {
        let text = format!(r#"{{"type":"Notification",{ENVELOPE},"data":{{"type":"FeatureCollection","features":[]}}}}"#);
        let Ok(Inbound::Notification(notification)) = inbound(&text) else {
            panic!("a notification");
        };
        assert!(matches!(notification.data, InboundData::FeatureCollection));
    }

    #[test]
    fn exact_numbers_survive() {
        let Ok(Inbound::Notification(notification)) = inbound(&format!(r#"{{"type":"Notification",{ENVELOPE},"data":[{VEHICLE}]}}"#)) else {
            panic!("a notification");
        };
        let InboundData::Entities(entities) = notification.data else {
            panic!("an entity array");
        };
        let [ReceivedElement::Read(contextual)] = entities.0.as_slice() else {
            panic!("one entity");
        };
        let written = serde_json::to_string(&contextual.entity).expect("writes");
        assert!(written.contains("1.1"), "{written}");
    }
}
