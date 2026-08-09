//! The URI identifying one temporal attribute instance.

use crate::uri::Uri;
use derive_more::{AsRef, Display, From, FromStr, Into};
use serde::{Deserialize, Serialize};

/// Identifier of one temporal Property or Relationship instance.
///
/// ETSI GS CIM 009 v1.9.1 clauses 5.6.14.4 and 5.6.15.4 require this value to
/// be a valid URI. The newtype prevents an entity, dataset, or registration URI
/// from being passed accidentally where an attribute instance is required.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, AsRef, Display, From, FromStr, Into)]
#[serde(transparent)]
#[as_ref(Uri)]
#[display("{_0}")]
pub struct InstanceId(Uri);

impl InstanceId {
    /// Borrows the validated URI.
    pub const fn uri(&self) -> &Uri {
        &self.0
    }

    /// Borrows the identifier's wire spelling.
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

#[cfg(test)]
mod tests {
    use crate::{instance_id::InstanceId, uri::Uri};

    #[test]
    fn an_instance_id_accepts_urn_and_https_uri_forms() {
        let urn = "urn:ngsi-ld:AttributeInstance:1".parse::<InstanceId>().expect("valid URN instance ID");
        let url = "https://example.com/instances/1".parse::<InstanceId>().expect("valid URL instance ID");

        assert!(matches!(urn.uri(), Uri::Urn(_)));
        assert!(matches!(url.uri(), Uri::Url(_)));
    }

    #[test]
    fn an_instance_id_rejects_bare_and_malformed_values() {
        assert!("instance-1".parse::<InstanceId>().is_err());
        assert!("https://[invalid".parse::<InstanceId>().is_err());
    }

    #[test]
    fn an_instance_id_preserves_its_exact_json_spelling() {
        let original = "https://example.com/instances/1".parse::<InstanceId>().expect("valid URL instance ID");
        let rendered = serde_json::to_string(&original).expect("serialises");
        let decoded = serde_json::from_str::<InstanceId>(&rendered).expect("deserialises");

        pretty_assertions::assert_eq!(decoded, original);
        pretty_assertions::assert_eq!(rendered, "\"https://example.com/instances/1\"");
    }
}
