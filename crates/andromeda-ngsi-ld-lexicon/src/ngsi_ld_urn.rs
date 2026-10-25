//! The `urn:ngsi-ld:<Kind>:<id>` identifiers the broker generates for its own resources.
//!
//! Every broker-minted resource kind has one typed identifier here, and each
//! type is the only place its identifier format is written. The shape follows
//! Annex A.3 of ETSI GS CIM 009 v1.9.1: the lowercase `urn:ngsi-ld:` prefix, a
//! `PascalCase` kind naming the resource, and exactly one colon-free local id.
//!
//! Which RFC 9562 UUID version a mint carries is decided per use:
//!
//! - version 4 for identifiers a client sees on low-volume resources, because a
//!   random identifier publishes nothing, where a version 7 one would publish
//!   its creation time and a version 1 or 6 one a node identifier;
//! - version 5 for identifiers derived from content, so equal content shares
//!   one identifier ([`JsonLdContextId::content_derived`]).
//!
//! Versions 1, 3 and 6 are never used: 1 and 6 embed a node identifier and 3
//! hashes with MD5. The high-volume and ordered identities take version 7 and
//! live beside the types they identify: write records and leases are bare
//! UUIDs, and temporal Attribute and scope instances are the lowercase
//! `urn:ngsi-ld:attribute:instance:<uuid>` family whose shape is permanent
//! ([`crate::instance_identity::InstanceIdentity`]).

use crate::{identifier::Identifier, identifier_length::IdentifierTooLong};
use derive_more::{AsRef, Deref, Display};
use iri_rs::{InvalidIri, IriBuf};
use serde::{Deserialize, Serialize, Serializer};
use std::str::FromStr;
use thiserror::Error;
use urn_rs::Urn;
use uuid::Uuid;

/// Declares one `urn:ngsi-ld:<Kind>:<id>` newtype over [`Urn`] together with its
/// parse-error enum.
///
/// Expands to a struct that serialises as its URN string and rejects, at
/// deserialisation, any value that is not a URN, does not use the `ngsi-ld`
/// namespace, does not carry the given kind as its first NSS segment, or is
/// longer than [`IDENTIFIER_BYTE_LIMIT`] -- the last so that every such
/// identifier converts into an [`Identifier`] without a second, fallible check.
///
/// [`IDENTIFIER_BYTE_LIMIT`]: crate::identifier_length::IDENTIFIER_BYTE_LIMIT
macro_rules! ngsi_ld_urn_id {
    ($name:ident, $error:ident, $constant:literal) => {
        #[doc = concat!("A `urn:ngsi-ld:", $constant, ":<id>` identifier.")]
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Display, AsRef, Deref)]
        #[display("{_0}")]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(Urn);

        #[doc = concat!("Why a value could not be read as a `urn:ngsi-ld:", $constant, ":<id>` identifier.")]
        #[derive(Debug, Error, Clone, PartialEq, Eq)]
        pub enum $error {
            /// The value is not a syntactically valid URN at all.
            #[error("invalid URN: {0}")]
            InvalidUrn(String),
            /// The URN names a different kind of resource.
            #[error("identifier must use the 'urn:ngsi-ld:{0}:' prefix", $constant)]
            WrongConstant,
            /// The URN carries the right prefix but no identifier after it.
            #[error("identifier suffix must not be empty")]
            EmptyId,
            /// The identifier itself contains a `:`, which would make the kind ambiguous.
            #[error("identifier suffix must not contain ':'")]
            ExtraColonInSuffix,
            /// The identifier is longer than any identifier may be.
            #[error(transparent)]
            TooLong(#[from] IdentifierTooLong),
        }

        impl $name {
            /// The resource kind this identifier names.
            pub const KIND: &'static str = $constant;
            const PREFIX: &'static str = concat!("urn:ngsi-ld:", $constant, ":");

            /// Borrow the identifier as the URN it wraps.
            pub const fn as_urn(&self) -> &Urn {
                &self.0
            }

            /// Mint a fresh identifier of this kind around a random (RFC 9562
            /// version 4) UUID.
            ///
            /// # Errors
            ///
            #[doc = concat!("Never in practice: a hyphenated UUID is a valid, colon-free local id well within the identifier bound, so every [`", stringify!($error), "`] variant is unreachable here.")]
            pub fn mint() -> Result<Self, $error> {
                Self::around_uuid(Uuid::new_v4())
            }

            /// The identifier of this kind whose local id is `uuid`, hyphenated.
            fn around_uuid(uuid: Uuid) -> Result<Self, $error> {
                Self::try_from(format!("{}{}", Self::PREFIX, uuid.hyphenated()))
            }
        }

        impl TryFrom<Urn> for $name {
            type Error = $error;

            fn try_from(value: Urn) -> Result<Self, Self::Error> {
                let raw = value.as_str();
                IdentifierTooLong::check(raw)?;
                if !raw.starts_with(Self::PREFIX) {
                    return Err($error::WrongConstant);
                }
                let suffix = &raw[Self::PREFIX.len()..];
                if suffix.is_empty() {
                    return Err($error::EmptyId);
                }
                if suffix.contains(':') {
                    return Err($error::ExtraColonInSuffix);
                }
                Ok(Self(value))
            }
        }

        impl TryFrom<&Urn> for $name {
            type Error = $error;

            fn try_from(value: &Urn) -> Result<Self, Self::Error> {
                value.as_str().try_into()
            }
        }

        impl TryFrom<String> for $name {
            type Error = $error;

            fn try_from(value: String) -> Result<Self, Self::Error> {
                let urn = match value.parse::<Urn>() {
                    Ok(value) => value,
                    Err(error) => return Err($error::InvalidUrn(error.to_string())),
                };
                urn.try_into()
            }
        }

        impl TryFrom<&str> for $name {
            type Error = $error;

            fn try_from(value: &str) -> Result<Self, Self::Error> {
                value.to_string().try_into()
            }
        }

        impl FromStr for $name {
            type Err = $error;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                s.try_into()
            }
        }

        impl From<$name> for Urn {
            fn from(value: $name) -> Urn {
                value.0
            }
        }

        impl From<$name> for String {
            fn from(value: $name) -> String {
                value.0.to_string()
            }
        }

        impl From<$name> for Identifier {
            fn from(value: $name) -> Identifier {
                Identifier::from_bounded_urn(value.0)
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                self.0.as_str()
            }
        }
    };
}

ngsi_ld_urn_id!(SnapshotId, SnapshotIdParseError, "Snapshot");
ngsi_ld_urn_id!(EntityMapId, EntityMapIdParseError, "EntityMap");
ngsi_ld_urn_id!(EntityTypeListId, EntityTypeListIdParseError, "EntityTypeList");
ngsi_ld_urn_id!(AttributeListId, AttributeListIdParseError, "AttributeList");
ngsi_ld_urn_id!(NotificationId, NotificationIdParseError, "Notification");
ngsi_ld_urn_id!(ContextSourceId, ContextSourceIdParseError, "ContextSourceIdentity");
ngsi_ld_urn_id!(SubscriptionId, SubscriptionIdParseError, "Subscription");
ngsi_ld_urn_id!(ContextSourceRegistrationId, ContextSourceRegistrationIdParseError, "ContextSourceRegistration");
ngsi_ld_urn_id!(HealthCheckId, HealthCheckIdParseError, "HealthCheck");
ngsi_ld_urn_id!(ComponentReportListId, ComponentReportListIdParseError, "ComponentReportList");
ngsi_ld_urn_id!(JsonLdContextId, JsonLdContextIdParseError, "JsonLdContext");

/// The namespace every content-derived [`JsonLdContextId`] is a version 5 UUID
/// in (RFC 9562 section 5.5).
///
/// Generated once and never to be changed. Changing it would not corrupt an
/// identifier already handed out, since a derived identifier is only ever
/// compared with the one the same content derives next; it would only stop new
/// derivations from meeting the entries stored under the old namespace, so
/// equal content would no longer share one entry until those entries are gone.
const IMPLICIT_CONTEXT_NAMESPACE: Uuid = Uuid::from_u128(0x85e3_3eb6_38dc_40ec_8e0a_4fc1_7ba5_0889);

/// The local-id marker of a subscription this broker hands to a peer.
const FORWARDED_SUBSCRIPTION_MARKER: &str = "forwarded-";

impl SubscriptionId {
    /// Mint the identifier of a subscription this broker hands to a peer: a
    /// companion or a forwarded copy.
    ///
    /// The marker sits inside the one local id (`forwarded-<uuid>`) rather than
    /// in a segment of its own, so the identifier keeps the single colon-free
    /// local id every broker-minted identifier has. The broker recognises the
    /// copies it forwarded by its route and mapping records, never by parsing
    /// this marker; the marker only tells a human reading a peer's subscription
    /// list where the subscription came from.
    ///
    /// # Errors
    ///
    /// Never in practice, for the reason [`SubscriptionId::mint`] gives.
    pub fn mint_forwarded() -> Result<SubscriptionId, SubscriptionIdParseError> {
        SubscriptionId::forwarded_around(Uuid::new_v4())
    }

    /// The forwarded-subscription identifier whose local id carries `uuid`.
    fn forwarded_around(uuid: Uuid) -> Result<SubscriptionId, SubscriptionIdParseError> {
        SubscriptionId::try_from(format!("{}{FORWARDED_SUBSCRIPTION_MARKER}{}", SubscriptionId::PREFIX, uuid.hyphenated()))
    }
}

impl JsonLdContextId {
    /// The identifier a stored `@context` document derives from its content.
    ///
    /// An `ImplicitlyCreated` entry (clause 5.13.1) is a document the broker
    /// hosts on its own initiative, so equal documents share one entry: the
    /// identifier is the version 5 UUID of `canonical_document` in
    /// [`IMPLICIT_CONTEXT_NAMESPACE`]. The caller hands the RFC 8785 canonical
    /// form of the document, so neither formatting, key order nor number
    /// spelling changes the identifier, while the order of an `@context` array,
    /// which JSON-LD processes in order, does.
    ///
    /// A version 5 UUID hashes with SHA-1, which does not resist a deliberate
    /// collision. The identifier is not a security boundary: an entry is shared
    /// only after its stored content is compared, so a collision can cost the
    /// sharing and never merge two documents.
    ///
    /// # Errors
    ///
    /// Never in practice, for the reason [`JsonLdContextId::mint`] gives.
    pub fn content_derived(canonical_document: &[u8]) -> Result<JsonLdContextId, JsonLdContextIdParseError> {
        JsonLdContextId::around_uuid(Uuid::new_v5(&IMPLICIT_CONTEXT_NAMESPACE, canonical_document))
    }

    /// The identifier as the IRI a JSON-LD processor names a context by.
    ///
    /// A URN is an IRI (IETF RFC 3987 section 2), so the conversion succeeds for
    /// every identifier of this kind; `every_json_ld_context_identifier_is_an_iri`
    /// proves it across minted and derived identifiers.
    ///
    /// # Errors
    ///
    /// Never in practice: the IRI grammar admits every URN this type holds.
    pub fn to_iri(&self) -> Result<IriBuf, InvalidIri<String>> {
        IriBuf::new(self.to_string())
    }
}

/// Identifier generated for a snapshot status notification.
///
/// Snapshot notification identifiers are broker-generated values, so retaining
/// their UUID rather than reparsing a string makes construction infallible.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Display)]
#[display("urn:ngsi-ld:SnapshotNotification:{_0}")]
pub struct SnapshotNotificationId(#[serde(serialize_with = "serialize_snapshot_notification_uuid")] Uuid);

impl SnapshotNotificationId {
    /// Mint a fresh identifier around a random (RFC 9562 version 4) UUID.
    ///
    /// A status notification is a low-volume, client-visible resource, so its
    /// identifier carries no information (see the module documentation).
    #[must_use]
    pub fn mint() -> SnapshotNotificationId {
        SnapshotNotificationId(Uuid::new_v4())
    }

    /// Create an identifier from the UUID generated for a notification.
    #[must_use]
    pub const fn from_uuid(uuid: Uuid) -> SnapshotNotificationId {
        SnapshotNotificationId(uuid)
    }

    /// Borrow the UUID that uniquely identifies this notification.
    #[must_use]
    pub const fn as_uuid(&self) -> &Uuid {
        &self.0
    }
}

/// Serialize the UUID payload with the snapshot-notification URN prefix.
fn serialize_snapshot_notification_uuid<S>(uuid: &Uuid, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.collect_str(&SnapshotNotificationId::from_uuid(*uuid))
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        identifier::Identifier,
        identifier_length::IDENTIFIER_BYTE_LIMIT,
        ngsi_ld_urn::{
            AttributeListId,
            ComponentReportListId,
            ContextSourceId,
            ContextSourceRegistrationId,
            EntityMapId,
            EntityTypeListId,
            HealthCheckId,
            JsonLdContextId,
            JsonLdContextIdParseError,
            NotificationId,
            SnapshotId,
            SnapshotNotificationId,
            SubscriptionId,
        },
    };
    use urn_rs::Urn;
    use uuid::{Uuid, Version};

    /// The UUID a minted identifier carries after its kind prefix.
    fn embedded_uuid(identifier: &str, prefix: &str) -> Uuid {
        let local_id = identifier.strip_prefix(prefix).expect("the identifier carries its kind prefix");
        Uuid::parse_str(local_id).expect("the local id is a hyphenated UUID")
    }

    #[test]
    fn a_json_ld_context_identifier_of_the_right_kind_is_accepted() {
        let raw = "urn:ngsi-ld:JsonLdContext:2fc7f63e-9213-4821-8720-65b9a404df6f";
        let id: JsonLdContextId = raw.parse().expect("a JsonLdContext identifier");
        pretty_assertions::assert_eq!(id.to_string(), raw);
    }

    #[test]
    fn a_json_ld_context_identifier_with_an_empty_local_id_is_refused() {
        pretty_assertions::assert_eq!("urn:ngsi-ld:JsonLdContext:".parse::<JsonLdContextId>(), Err(JsonLdContextIdParseError::EmptyId));
    }

    #[test]
    fn a_json_ld_context_identifier_with_a_colon_in_its_local_id_is_refused() {
        pretty_assertions::assert_eq!(
            "urn:ngsi-ld:JsonLdContext:a:b".parse::<JsonLdContextId>(),
            Err(JsonLdContextIdParseError::ExtraColonInSuffix)
        );
    }

    /// The retired `Context` kind names no stored `@context` any more.
    #[test]
    fn the_retired_context_kind_is_not_a_json_ld_context_identifier() {
        pretty_assertions::assert_eq!(
            "urn:ngsi-ld:Context:x".parse::<JsonLdContextId>(),
            Err(JsonLdContextIdParseError::WrongConstant)
        );
    }

    /// RFC 8141 section 3.1 compares `urn` and the NID case-insensitively, and
    /// the URN parser folds them to lowercase, so a typed identifier only ever
    /// holds the lowercase prefix Annex A.3 asks for: an uppercase spelling
    /// reads as the canonical identifier rather than as a second one.
    #[test]
    fn a_non_canonical_prefix_spelling_reads_as_the_canonical_identifier() {
        let folded: JsonLdContextId = "URN:NGSI-LD:JsonLdContext:x".parse().expect("the prefix compares case-insensitively");
        pretty_assertions::assert_eq!(folded.to_string(), "urn:ngsi-ld:JsonLdContext:x");
    }

    /// The kind and the local id are the NSS, which compares case-sensitively.
    #[test]
    fn the_kind_is_matched_case_sensitively() {
        pretty_assertions::assert_eq!(
            "urn:ngsi-ld:jsonldcontext:x".parse::<JsonLdContextId>(),
            Err(JsonLdContextIdParseError::WrongConstant)
        );
    }

    #[test]
    fn a_url_is_not_a_json_ld_context_identifier() {
        assert!(matches!(
            "https://example.org/ctx.jsonld".parse::<JsonLdContextId>(),
            Err(JsonLdContextIdParseError::InvalidUrn(_))
        ));
    }

    #[test]
    fn a_json_ld_context_identifier_is_bounded_like_every_identifier() {
        let prefix = "urn:ngsi-ld:JsonLdContext:";
        let at_limit = format!("{prefix}{}", "a".repeat(IDENTIFIER_BYTE_LIMIT - prefix.len()));
        let past_limit = format!("{prefix}{}", "a".repeat(IDENTIFIER_BYTE_LIMIT + 1 - prefix.len()));

        assert!(at_limit.parse::<JsonLdContextId>().is_ok());
        assert!(matches!(past_limit.parse::<JsonLdContextId>(), Err(JsonLdContextIdParseError::TooLong(_))));
    }

    /// A Hosted or Cached entry gets a random identifier.
    #[test]
    fn a_minted_json_ld_context_identifier_carries_a_version_4_uuid() {
        let id = JsonLdContextId::mint().expect("a minted identifier");
        let reparsed: JsonLdContextId = id.to_string().parse().expect("a minted identifier parses back");

        pretty_assertions::assert_eq!(reparsed, id);
        pretty_assertions::assert_eq!(embedded_uuid(id.as_ref(), "urn:ngsi-ld:JsonLdContext:").get_version(), Some(Version::Random));
    }

    /// Equal content derives an equal identifier, and the identifier carries a
    /// version 5 UUID.
    #[test]
    fn equal_content_derives_an_equal_version_5_identifier() {
        let document = br#"{"@context":{"a":"https://e.org/a","b":"https://e.org/b"}}"#;
        let first = JsonLdContextId::content_derived(document).expect("a derived identifier");
        let second = JsonLdContextId::content_derived(document).expect("a derived identifier");

        pretty_assertions::assert_eq!(first, second);
        pretty_assertions::assert_eq!(embedded_uuid(first.as_ref(), "urn:ngsi-ld:JsonLdContext:").get_version(), Some(Version::Sha1));
    }

    #[test]
    fn different_content_derives_a_different_identifier() {
        let first = JsonLdContextId::content_derived(br#"{"@context":{"a":"https://e.org/a"}}"#).expect("a derived identifier");
        let second = JsonLdContextId::content_derived(br#"{"@context":{"a":"https://e.org/b"}}"#).expect("a derived identifier");

        assert_ne!(first, second);
    }

    /// The namespace is fixed: this pins the identifier one known document
    /// derives, so a change to the namespace constant fails here rather than
    /// silently splitting equal documents into two entries.
    #[test]
    fn the_derivation_namespace_is_stable() {
        let id = JsonLdContextId::content_derived(br#"{"@context":{}}"#).expect("a derived identifier");
        let expected = Uuid::new_v5(
            &Uuid::parse_str("85e33eb6-38dc-40ec-8e0a-4fc17ba50889").expect("the namespace literal"),
            br#"{"@context":{}}"#,
        );

        pretty_assertions::assert_eq!(id.to_string(), format!("urn:ngsi-ld:JsonLdContext:{expected}"));
    }

    /// Every broker-minted kind mints a version 4 identifier under its own
    /// prefix that converts into an [`Identifier`].
    #[test]
    fn every_minted_kind_carries_its_prefix_and_a_version_4_uuid() {
        let minted: [(String, &str); 9] = [
            (SubscriptionId::mint().expect("mint").to_string(), "urn:ngsi-ld:Subscription:"),
            (
                ContextSourceRegistrationId::mint().expect("mint").to_string(),
                "urn:ngsi-ld:ContextSourceRegistration:",
            ),
            (SnapshotId::mint().expect("mint").to_string(), "urn:ngsi-ld:Snapshot:"),
            (EntityMapId::mint().expect("mint").to_string(), "urn:ngsi-ld:EntityMap:"),
            (NotificationId::mint().expect("mint").to_string(), "urn:ngsi-ld:Notification:"),
            (EntityTypeListId::mint().expect("mint").to_string(), "urn:ngsi-ld:EntityTypeList:"),
            (AttributeListId::mint().expect("mint").to_string(), "urn:ngsi-ld:AttributeList:"),
            (HealthCheckId::mint().expect("mint").to_string(), "urn:ngsi-ld:HealthCheck:"),
            (ComponentReportListId::mint().expect("mint").to_string(), "urn:ngsi-ld:ComponentReportList:"),
        ];
        for (identifier, prefix) in minted {
            pretty_assertions::assert_eq!(embedded_uuid(&identifier, prefix).get_version(), Some(Version::Random), "{identifier}");
            assert!(identifier.parse::<Identifier>().is_ok(), "{identifier} converts into an identifier");
        }
    }

    /// The forwarded marker lives inside the one local id, so a forwarded
    /// subscription identifier still has no colon after its kind.
    #[test]
    fn a_forwarded_subscription_identifier_keeps_a_single_local_id() {
        let id = SubscriptionId::mint_forwarded().expect("a forwarded identifier").to_string();
        let local_id = id
            .strip_prefix("urn:ngsi-ld:Subscription:forwarded-")
            .expect("the forwarded marker opens the local id");

        pretty_assertions::assert_eq!(Uuid::parse_str(local_id).expect("a hyphenated UUID").get_version(), Some(Version::Random));
    }

    #[test]
    fn a_minted_snapshot_notification_identifier_carries_a_version_4_uuid() {
        pretty_assertions::assert_eq!(SnapshotNotificationId::mint().as_uuid().get_version(), Some(Version::Random));
    }

    #[test]
    fn snapshot_id_accepts_valid_value() {
        let id: SnapshotId = "urn:ngsi-ld:Snapshot:abc".parse().expect("valid snapshot id");
        pretty_assertions::assert_eq!(id.to_string(), "urn:ngsi-ld:Snapshot:abc");
    }

    #[test]
    fn snapshot_id_rejects_wrong_constant() {
        let parsed = "urn:ngsi-ld:EntityMap:abc".parse::<SnapshotId>();
        assert!(parsed.is_err());
    }

    #[test]
    fn snapshot_id_rejects_empty_suffix() {
        let parsed = "urn:ngsi-ld:Snapshot:".parse::<SnapshotId>();
        assert!(parsed.is_err());
    }

    #[test]
    fn snapshot_id_rejects_extra_colon() {
        let parsed = "urn:ngsi-ld:Snapshot:abc:def".parse::<SnapshotId>();
        assert!(parsed.is_err());
    }

    #[test]
    fn serde_round_trip() {
        let id: EntityMapId = "urn:ngsi-ld:EntityMap:test".parse().expect("valid entity map id");
        let serialized = serde_json::to_string(&id).expect("serialize id");
        let decoded: EntityMapId = serde_json::from_str(&serialized).expect("deserialize id");
        pretty_assertions::assert_eq!(decoded, id);
    }

    #[test]
    fn try_from_urn() {
        let urn = "urn:ngsi-ld:AttributeList:1".parse::<Urn>().expect("valid urn");
        let id = AttributeListId::try_from(urn).expect("valid typed id");
        pretty_assertions::assert_eq!(id.to_string(), "urn:ngsi-ld:AttributeList:1");
    }

    #[test]
    fn all_types_parse() {
        assert!("urn:ngsi-ld:EntityTypeList:t1".parse::<EntityTypeListId>().is_ok());
        assert!("urn:ngsi-ld:Notification:n1".parse::<NotificationId>().is_ok());
        assert!("urn:ngsi-ld:ContextSourceIdentity:cs1".parse::<ContextSourceId>().is_ok());
    }

    #[test]
    fn snapshot_notification_id_renders_the_uuid_urn() {
        let uuid = Uuid::parse_str("123e4567-e89b-12d3-a456-426614174000").expect("valid UUID fixture");
        let id = SnapshotNotificationId::from_uuid(uuid);

        pretty_assertions::assert_eq!(id.to_string(), "urn:ngsi-ld:SnapshotNotification:123e4567-e89b-12d3-a456-426614174000");
        pretty_assertions::assert_eq!(
            serde_json::to_value(id).expect("serializable identifier"),
            "urn:ngsi-ld:SnapshotNotification:123e4567-e89b-12d3-a456-426614174000"
        );
    }

    #[test]
    fn notification_and_context_source_reject_wrong_constant() {
        assert!("urn:ngsi-ld:Snapshot:x".parse::<NotificationId>().is_err());
        assert!("urn:ngsi-ld:Notification:x".parse::<ContextSourceId>().is_err());
    }

    /// Every UUID a mint can meet: the nil and the max UUID, then ten thousand
    /// random ones of the three versions the broker mints (4, 5 and 7).
    fn every_uuid_a_mint_can_meet() -> Vec<Uuid> {
        let mut uuids = vec![Uuid::nil(), Uuid::max()];
        uuids.extend((0_u32..10_000).map(|index| match index % 3 {
            0 => Uuid::new_v4(),
            1 => Uuid::new_v5(&Uuid::NAMESPACE_URL, &index.to_be_bytes()),
            _ => Uuid::now_v7(),
        }));
        uuids
    }

    /// Mint `uuid` as a `$kind` identifier and require it to parse back to
    /// itself.
    macro_rules! assert_round_trip {
        ($kind:ty, $minted:expr) => {{
            let minted: $kind = $minted.expect(concat!("a ", stringify!($kind), " is minted"));
            let reparsed: $kind = minted.to_string().parse().expect(concat!("a minted ", stringify!($kind), " parses back"));
            assert_eq!(reparsed, minted);
        }};
    }

    /// The "never in practice" of every mint's error is proven rather than
    /// asserted: every kind mints and round-trips through `FromStr` over the
    /// nil UUID, the max UUID and ten thousand random ones, and so do the
    /// forwarded-subscription and content-derived mints.
    #[test]
    fn every_kind_mints_and_round_trips_over_every_uuid() {
        for uuid in every_uuid_a_mint_can_meet() {
            assert_round_trip!(SnapshotId, SnapshotId::around_uuid(uuid));
            assert_round_trip!(EntityMapId, EntityMapId::around_uuid(uuid));
            assert_round_trip!(EntityTypeListId, EntityTypeListId::around_uuid(uuid));
            assert_round_trip!(AttributeListId, AttributeListId::around_uuid(uuid));
            assert_round_trip!(NotificationId, NotificationId::around_uuid(uuid));
            assert_round_trip!(ContextSourceId, ContextSourceId::around_uuid(uuid));
            assert_round_trip!(SubscriptionId, SubscriptionId::around_uuid(uuid));
            assert_round_trip!(SubscriptionId, SubscriptionId::forwarded_around(uuid));
            assert_round_trip!(ContextSourceRegistrationId, ContextSourceRegistrationId::around_uuid(uuid));
            assert_round_trip!(HealthCheckId, HealthCheckId::around_uuid(uuid));
            assert_round_trip!(ComponentReportListId, ComponentReportListId::around_uuid(uuid));
            assert_round_trip!(JsonLdContextId, JsonLdContextId::around_uuid(uuid));
            assert_round_trip!(JsonLdContextId, JsonLdContextId::content_derived(uuid.as_bytes()));
        }
    }

    /// A stored `@context` is named by its identifier as an IRI; every minted
    /// and derived identifier converts, spelled exactly as it displays.
    #[test]
    fn every_json_ld_context_identifier_is_an_iri() {
        for uuid in every_uuid_a_mint_can_meet() {
            for identifier in [
                JsonLdContextId::around_uuid(uuid).expect("a minted identifier"),
                JsonLdContextId::content_derived(uuid.as_bytes()).expect("a derived identifier"),
            ] {
                let iri = identifier.to_iri().expect("a URN is an IRI");
                pretty_assertions::assert_eq!(iri.as_str(), identifier.to_string());
            }
        }
    }
}
