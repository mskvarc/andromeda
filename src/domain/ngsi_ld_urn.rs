use crate::domain::uri::Uri;
use derive_more::{AsRef, Deref, Display};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use urn::Urn;

macro_rules! ngsi_ld_urn_id {
    ($name:ident, $error:ident, $constant:literal) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Display, AsRef, Deref)]
        #[display("{_0}")]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(Urn);

        #[derive(Debug, Error, Clone, PartialEq, Eq)]
        pub enum $error {
            #[error("invalid URN: {0}")]
            InvalidUrn(String),
            #[error("identifier must use the 'urn:ngsi-ld:{0}:' prefix", $constant)]
            WrongConstant,
            #[error("identifier suffix must not be empty")]
            EmptyId,
            #[error("identifier suffix must not contain ':'")]
            ExtraColonInSuffix,
        }

        impl $name {
            pub const KIND: &'static str = $constant;
            const PREFIX: &'static str = concat!("urn:ngsi-ld:", $constant, ":");

            pub fn as_urn(&self) -> &Urn {
                &self.0
            }
        }

        impl TryFrom<Urn> for $name {
            type Error = $error;

            fn try_from(value: Urn) -> Result<Self, Self::Error> {
                let raw = value.as_str();
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

        impl std::str::FromStr for $name {
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
                Uri::Urn(value.0)
            }
        }
    };
}

ngsi_ld_urn_id!(SnapshotId, SnapshotIdParseError, "Snapshot");
ngsi_ld_urn_id!(SnapshotNotificationId, SnapshotNotificationIdParseError, "SnapshotNotification");
ngsi_ld_urn_id!(EntityMapId, EntityMapIdParseError, "EntityMap");
ngsi_ld_urn_id!(EntityTypeListId, EntityTypeListIdParseError, "EntityTypeList");
ngsi_ld_urn_id!(AttributeListId, AttributeListIdParseError, "AttributeList");

#[cfg(test)]
mod tests {
    use crate::domain::ngsi_ld_urn::{AttributeListId, EntityMapId, EntityTypeListId, SnapshotId, SnapshotNotificationId};
    use urn::Urn;

    #[test]
    fn snapshot_id_accepts_valid_value() {
        let id: SnapshotId = "urn:ngsi-ld:Snapshot:abc".parse().expect("valid snapshot id");
        assert_eq!(id.to_string(), "urn:ngsi-ld:Snapshot:abc");
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
        assert_eq!(decoded, id);
    }

    #[test]
    fn try_from_urn() {
        let urn = "urn:ngsi-ld:AttributeList:1".parse::<Urn>().expect("valid urn");
        let id = AttributeListId::try_from(urn).expect("valid typed id");
        assert_eq!(id.to_string(), "urn:ngsi-ld:AttributeList:1");
    }

    #[test]
    fn all_types_parse() {
        assert!("urn:ngsi-ld:SnapshotNotification:n1".parse::<SnapshotNotificationId>().is_ok());
        assert!("urn:ngsi-ld:EntityTypeList:t1".parse::<EntityTypeListId>().is_ok());
    }
}
