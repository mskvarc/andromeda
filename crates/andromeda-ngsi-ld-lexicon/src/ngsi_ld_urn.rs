//! The `urn:ngsi-ld:<Kind>:<id>` identifiers the broker generates for its own resources.

use crate::{identifier_length::IdentifierTooLong, uri::Uri};
use derive_more::{AsRef, Deref, Display};
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
/// identifier converts into a [`Uri`] without a second, fallible check.
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

        impl From<$name> for Uri {
            fn from(value: $name) -> Uri {
                Uri::from_bounded_urn(value.0)
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

/// Identifier generated for a snapshot status notification.
///
/// Snapshot notification identifiers are broker-generated values, so retaining
/// their UUID rather than reparsing a string makes construction infallible.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Display)]
#[display("urn:ngsi-ld:SnapshotNotification:{_0}")]
pub struct SnapshotNotificationId(#[serde(serialize_with = "serialize_snapshot_notification_uuid")] Uuid);

impl SnapshotNotificationId {
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
mod tests {
    use crate::ngsi_ld_urn::{AttributeListId, ContextSourceId, EntityMapId, EntityTypeListId, NotificationId, SnapshotId, SnapshotNotificationId};
    use urn_rs::Urn;
    use uuid::Uuid;

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
}
