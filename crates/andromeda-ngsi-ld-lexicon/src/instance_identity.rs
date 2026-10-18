//! The two shapes a temporal instance identifier is held in.

use crate::{identifier::Identifier, uri::NGSI_LD_URN_NID};
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
/// through the permanently stable template. A client-supplied
/// identifier — and any older broker-minted spelling the template cannot
/// reproduce byte for byte — is kept verbatim, because re-rendering it would
/// hand a client a different identifier than the one it already holds.
#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum InstanceIdentity {
    /// The broker minted the identifier and keeps only its UUID.
    Generated(Uuid),
    /// The identifier is kept exactly as it was received.
    Verbatim(Identifier),
}

impl InstanceIdentity {
    /// Mint an identifier for an instance the client did not name.
    ///
    /// The UUID is version 7, so minted identifiers sort by the instant they
    /// were minted and cluster in the index that carries them. That is why this
    /// client-visible identifier departs from the version 4 every other
    /// broker-minted resource URN carries ([`crate::ngsi_ld_urn`]): one is
    /// written per history row, the index locality is worth more than hiding
    /// the mint time, and the instance already carries its own `createdAt` and
    /// `observedAt`.
    ///
    /// The rendered identifier (`urn:ngsi-ld:attribute:instance:<uuid>`) is
    /// also the one deliberate exception to the `urn:ngsi-ld:<Kind>:<id>`
    /// shape: its kind is lowercase and two segments deep. The exception is
    /// permanent. The template must keep rendering an identifier already handed
    /// to a client byte for byte, so it is never "fixed" to the `PascalCase`,
    /// single-segment shape.
    #[must_use]
    pub fn generate() -> InstanceIdentity {
        InstanceIdentity::Generated(Uuid::now_v7())
    }

    /// Read an identifier back from the URI it was received or stored as.
    ///
    /// An identifier is taken as [`Generated`](InstanceIdentity::Generated)
    /// only when rendering its UUID through the template reproduces
    /// the input exactly. That test, rather than a prefix match, is what makes
    /// the split lossless: an identifier that merely resembles the template — a
    /// non-canonical UUID spelling, a URN carrying a resolution component, a
    /// trailing segment such as a write ordinal — is kept verbatim instead of
    /// being silently rewritten.
    #[must_use]
    pub fn from_identifier(identifier: Identifier) -> InstanceIdentity {
        match generated_uuid_of(&identifier) {
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
    pub const fn rendered(&self) -> RenderedInstanceId<'_> {
        match self {
            InstanceIdentity::Generated(uuid) => RenderedInstanceId::Generated(*uuid),
            InstanceIdentity::Verbatim(identifier) => RenderedInstanceId::Verbatim(identifier),
        }
    }
}

/// One temporal instance identifier in the spelling a client receives.
///
/// The generated arm carries the permanently stable template. The template is
/// never changed and never stored: a generated identifier is spelled out from
/// its UUID every time it is rendered.
#[derive(Clone, Copy, Debug, Display, Eq, Hash, PartialEq)]
pub enum RenderedInstanceId<'a> {
    /// A broker-minted identifier, spelled through the template.
    #[display("urn:{NGSI_LD_URN_NID}:{GENERATED_NSS_PREFIX}{}", _0.hyphenated())]
    Generated(Uuid),
    /// A client-supplied identifier, in the exact spelling it arrived in.
    #[display("{_0}")]
    Verbatim(&'a Identifier),
}

/// The namespace-specific prefix a broker-minted identifier carries.
const GENERATED_NSS_PREFIX: &str = "attribute:instance:";

/// The UUID a generated identifier carries, when the template reproduces it exactly.
fn generated_uuid_of(identifier: &Identifier) -> Option<Uuid> {
    let urn = identifier.as_urn()?;
    if urn.nid() != NGSI_LD_URN_NID {
        return None;
    }
    let candidate = urn.nss().strip_prefix(GENERATED_NSS_PREFIX)?;
    let uuid = Uuid::try_parse(candidate).ok()?;
    (RenderedInstanceId::Generated(uuid).to_string() == identifier.as_str()).then_some(uuid)
}

#[cfg(test)]
mod tests {
    use crate::{
        identifier::Identifier,
        instance_identity::{InstanceIdentity, RenderedInstanceId},
    };
    use uuid::Uuid;

    /// Parse a fixture identifier, reporting the text when it does not parse.
    fn uri(text: &str) -> Result<Identifier, String> {
        text.parse::<Identifier>().map_err(|_| format!("{text} is not an identifier"))
    }

    /// Decode a fixture identifier and render it back out.
    fn round_trip(text: &str) -> Result<String, String> {
        uri(text).map(|identifier| InstanceIdentity::from_identifier(identifier).rendered().to_string())
    }

    #[test]
    fn a_generated_identifier_renders_through_the_permanent_template() {
        let uuid = Uuid::from_u128(0x0194_2f6c_7f00_7000_8000_0000_0000_0001);
        pretty_assertions::assert_eq!(
            RenderedInstanceId::Generated(uuid).to_string(),
            "urn:ngsi-ld:attribute:instance:01942f6c-7f00-7000-8000-000000000001"
        );
    }

    #[test]
    fn a_minted_identifier_decodes_back_to_the_uuid_it_was_minted_from() {
        let minted = InstanceIdentity::generate();
        let rendered = minted.rendered().to_string();
        let decoded = uri(&rendered).map(InstanceIdentity::from_identifier);
        pretty_assertions::assert_eq!(decoded, Ok(minted), "{rendered} must decode as generated");
    }

    #[test]
    fn a_client_supplied_identifier_is_kept_verbatim() {
        let identifier = uri("urn:ngsi-ld:my-own:instance:42");
        let decoded = identifier.clone().map(InstanceIdentity::from_identifier);

        pretty_assertions::assert_eq!(decoded, identifier.map(InstanceIdentity::Verbatim));
    }

    #[test]
    fn a_url_identifier_is_kept_verbatim() {
        let identifier = uri("https://example.org/instances/1");
        let decoded = identifier.clone().map(InstanceIdentity::from_identifier);

        pretty_assertions::assert_eq!(decoded, identifier.map(InstanceIdentity::Verbatim));
    }

    #[test]
    fn an_identifier_under_another_template_is_kept_verbatim() {
        let identifier = uri("urn:ngsi-ld:scope:instance:01942f6c-7f00-7000-8000-000000000001");
        let decoded = identifier.clone().map(InstanceIdentity::from_identifier);

        pretty_assertions::assert_eq!(decoded, identifier.map(InstanceIdentity::Verbatim));
    }

    #[test]
    fn every_identifier_the_template_cannot_reproduce_survives_the_round_trip_unchanged() {
        for text in [
            "urn:ngsi-ld:my-own:instance:42",
            "https://example.org/instances/1",
            "urn:ngsi-ld:attribute:instance:not-a-uuid",
            // A write identifier plus an ordinal: the template holds one value,
            // so re-rendering it would change an identifier clients hold.
            "urn:ngsi-ld:attribute:instance:01942f6c-7f00-7000-8000-000000000001:0",
            "urn:ngsi-ld:attribute:instance:01942F6C-7F00-7000-8000-000000000001",
            "urn:ngsi-ld:attribute:instance:01942f6c7f0070008000000000000001",
        ] {
            pretty_assertions::assert_eq!(round_trip(text), Ok(text.to_owned()), "{text} must not be rewritten");
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
