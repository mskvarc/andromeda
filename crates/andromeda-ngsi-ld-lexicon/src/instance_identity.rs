//! The two shapes a temporal instance identifier is held in.

use crate::{
    instance_family::InstanceFamily,
    uri::{NGSI_LD_URN_NID, Uri},
};
use derive_more::Display;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// How one temporal instance identifier is held between storage and rendering.
///
/// Clause 4.5.7 says a system should maintain an `instanceId`: a client may
/// supply its own and the broker should keep it, while the broker mints one
/// for every instance a client did not name. Holding both as text would lose
/// the fact that the far more common broker-minted case is a UUID, and would
/// repeat the URN prefix on every history row, so the two populations are held
/// apart and one instance carries exactly one of them.
///
/// A broker-minted identifier keeps only its [`Uuid`] and is re-rendered
/// through the family's permanently stable template. A client-supplied
/// identifier — and any older broker-minted spelling the template cannot
/// reproduce byte for byte — is kept verbatim, because re-rendering it would
/// hand a client a different identifier than the one it already holds.
#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum InstanceIdentity {
    /// The broker minted the identifier and keeps only its UUID.
    Generated(Uuid),
    /// The identifier is kept exactly as it was received.
    Verbatim(Uri),
}

impl InstanceIdentity {
    /// Mint an identifier for an instance the client did not name.
    ///
    /// The UUID is version 7, so minted identifiers sort by the instant they
    /// were minted and cluster in the index that carries them.
    #[must_use]
    pub fn generate() -> InstanceIdentity {
        InstanceIdentity::Generated(Uuid::now_v7())
    }

    /// Read an identifier back from the URI it was received or stored as.
    ///
    /// An identifier is taken as [`Generated`](InstanceIdentity::Generated)
    /// only when rendering its UUID through the family's template reproduces
    /// the input exactly. That test, rather than a prefix match, is what makes
    /// the split lossless: an identifier that merely resembles the template — a
    /// non-canonical UUID spelling, a URN carrying a resolution component, a
    /// trailing segment such as the write ordinal earlier releases appended —
    /// is kept verbatim instead of being silently rewritten.
    #[must_use]
    pub fn from_uri(identifier: Uri, family: InstanceFamily) -> InstanceIdentity {
        match generated_uuid_of(&identifier, family) {
            Some(uuid) => InstanceIdentity::Generated(uuid),
            None => InstanceIdentity::Verbatim(identifier),
        }
    }

    /// The identifier a client sees for this instance.
    ///
    /// Rendering is total: a generated identifier is spelled out from its UUID
    /// rather than reparsed, so no stored value can make an instance
    /// unrenderable.
    #[must_use]
    pub const fn rendered(&self, family: InstanceFamily) -> RenderedInstanceId<'_> {
        match self {
            InstanceIdentity::Generated(uuid) => RenderedInstanceId::Generated(family, *uuid),
            InstanceIdentity::Verbatim(identifier) => RenderedInstanceId::Verbatim(identifier),
        }
    }
}

/// One temporal instance identifier in the spelling a client receives.
///
/// The generated arm carries the family's permanently stable template. Neither
/// template is ever changed, and neither is stored: a generated identifier is
/// spelled out from its UUID every time it is rendered.
#[derive(Clone, Copy, Debug, Display, Eq, Hash, PartialEq)]
pub enum RenderedInstanceId<'a> {
    /// A broker-minted identifier, spelled through its family's template.
    #[display("urn:{NGSI_LD_URN_NID}:{}{}", _0.nss_prefix(), _1.hyphenated())]
    Generated(InstanceFamily, Uuid),
    /// A client-supplied identifier, in the exact spelling it arrived in.
    #[display("{_0}")]
    Verbatim(&'a Uri),
}

/// The UUID a generated identifier carries, when the template reproduces it exactly.
fn generated_uuid_of(identifier: &Uri, family: InstanceFamily) -> Option<Uuid> {
    let urn = identifier.as_urn()?;
    if urn.nid() != NGSI_LD_URN_NID {
        return None;
    }
    let candidate = urn.nss().strip_prefix(family.nss_prefix())?;
    let uuid = Uuid::try_parse(candidate).ok()?;
    (RenderedInstanceId::Generated(family, uuid).to_string() == identifier.as_str()).then_some(uuid)
}

#[cfg(test)]
mod tests {
    use crate::{
        instance_family::InstanceFamily,
        instance_identity::{InstanceIdentity, RenderedInstanceId},
        uri::Uri,
    };
    use uuid::Uuid;

    /// Parse a fixture identifier, reporting the text when it does not parse.
    fn uri(text: &str) -> Result<Uri, String> {
        text.parse::<Uri>().map_err(|_| format!("{text} is not a URI"))
    }

    /// Decode a fixture identifier and render it back out.
    fn round_trip(text: &str, family: InstanceFamily) -> Result<String, String> {
        uri(text).map(|identifier| InstanceIdentity::from_uri(identifier, family).rendered(family).to_string())
    }

    #[test]
    fn a_generated_attribute_identifier_renders_through_the_permanent_template() {
        let uuid = Uuid::from_u128(0x0194_2f6c_7f00_7000_8000_0000_0000_0001);
        pretty_assertions::assert_eq!(
            RenderedInstanceId::Generated(InstanceFamily::Attribute, uuid).to_string(),
            "urn:ngsi-ld:attribute:instance:01942f6c-7f00-7000-8000-000000000001"
        );
    }

    #[test]
    fn a_generated_scope_identifier_renders_through_its_own_template() {
        let uuid = Uuid::from_u128(0x0194_2f6c_7f00_7000_8000_0000_0000_0001);
        pretty_assertions::assert_eq!(
            RenderedInstanceId::Generated(InstanceFamily::Scope, uuid).to_string(),
            "urn:ngsi-ld:scope:instance:01942f6c-7f00-7000-8000-000000000001"
        );
    }

    #[test]
    fn a_minted_identifier_decodes_back_to_the_uuid_it_was_minted_from() {
        let minted = InstanceIdentity::generate();
        for family in [InstanceFamily::Attribute, InstanceFamily::Scope] {
            let rendered = minted.rendered(family).to_string();
            let decoded = uri(&rendered).map(|identifier| InstanceIdentity::from_uri(identifier, family));
            pretty_assertions::assert_eq!(decoded, Ok(minted.clone()), "{rendered} must decode as generated");
        }
    }

    #[test]
    fn a_client_supplied_identifier_is_kept_verbatim() {
        let identifier = uri("urn:ngsi-ld:my-own:instance:42");
        let decoded = identifier
            .clone()
            .map(|identifier| InstanceIdentity::from_uri(identifier, InstanceFamily::Attribute));

        pretty_assertions::assert_eq!(decoded, identifier.map(InstanceIdentity::Verbatim));
    }

    #[test]
    fn a_url_identifier_is_kept_verbatim() {
        let identifier = uri("https://example.org/instances/1");
        let decoded = identifier
            .clone()
            .map(|identifier| InstanceIdentity::from_uri(identifier, InstanceFamily::Attribute));

        pretty_assertions::assert_eq!(decoded, identifier.map(InstanceIdentity::Verbatim));
    }

    #[test]
    fn a_scope_identifier_does_not_decode_as_an_attribute_identifier() {
        let identifier = uri("urn:ngsi-ld:scope:instance:01942f6c-7f00-7000-8000-000000000001");
        let decoded = identifier
            .clone()
            .map(|identifier| InstanceIdentity::from_uri(identifier, InstanceFamily::Attribute));

        pretty_assertions::assert_eq!(decoded, identifier.map(InstanceIdentity::Verbatim));
    }

    #[test]
    fn every_identifier_the_template_cannot_reproduce_survives_the_round_trip_unchanged() {
        for text in [
            "urn:ngsi-ld:my-own:instance:42",
            "https://example.org/instances/1",
            "urn:ngsi-ld:attribute:instance:not-a-uuid",
            // The spelling earlier releases minted held a write identifier and
            // an ordinal; the template holds one value, so re-rendering it
            // would change an identifier clients already hold.
            "urn:ngsi-ld:attribute:instance:01942f6c-7f00-7000-8000-000000000001:0",
            "urn:ngsi-ld:attribute:instance:01942F6C-7F00-7000-8000-000000000001",
            "urn:ngsi-ld:attribute:instance:01942f6c7f0070008000000000000001",
        ] {
            pretty_assertions::assert_eq!(round_trip(text, InstanceFamily::Attribute), Ok(text.to_owned()), "{text} must not be rewritten");
        }
    }

    #[test]
    fn a_generated_identifier_serialises_as_its_uuid_and_a_verbatim_one_as_its_uri() {
        let uuid = Uuid::from_u128(0x0194_2f6c_7f00_7000_8000_0000_0000_0001);
        let generated = serde_json::to_value(InstanceIdentity::Generated(uuid)).map_err(|error| error.to_string());
        let verbatim = uri("https://example.org/instances/1")
            .map(InstanceIdentity::Verbatim)
            .and_then(|identity| serde_json::to_value(identity).map_err(|error| error.to_string()));

        pretty_assertions::assert_eq!(generated, Ok(serde_json::json!({ "generated": "01942f6c-7f00-7000-8000-000000000001" })));
        pretty_assertions::assert_eq!(verbatim, Ok(serde_json::json!({ "verbatim": "https://example.org/instances/1" })));
    }
}
