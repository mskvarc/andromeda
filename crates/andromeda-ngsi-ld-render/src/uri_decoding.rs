//! Percent-decoding of the entity identifiers Relationships target.
//!
//! An entity identifier is a URI, and a client is free to percent-encode
//! characters in one when it writes a Relationship. The store keeps what it was
//! given, so the decoding happens at the presentation boundary, and it reaches
//! exactly the members that hold a target: a Relationship's `object` and
//! `previousObject`, and a `ListRelationship`'s `objectList` and
//! `previousObjectList`, wherever the attribute instance stands -- at the top
//! of an entity, as a sub-attribute, or inside an entity an inline join nested
//! (clause 4.5.23.2). A client's own value -- a Property's `value`, a
//! `JsonProperty`'s `json` -- is never decoded, whatever it holds, and no
//! target is decoded twice.
//!
//! The typed walk of [`crate::entity_body`] applies the rule as it writes each
//! member, through [`UriDecoding`] and [`DecodedTargets`]; a target is a typed
//! [`Uri`] there, so nothing has to tell a target from a value by its shape.

use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::attribute::one_or_many::OneOrMany;
use serde::{Serialize, Serializer};
use std::borrow::Cow;

/// Whether the presentation boundary percent-decodes Relationship targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UriDecoding {
    /// Every target is written decoded.
    Decode,
    /// Every target is written as the store holds it.
    Keep,
}

/// The targets of one Relationship member, written under a [`UriDecoding`].
#[derive(Debug, Clone, Copy)]
pub(crate) struct DecodedTargets<'a> {
    /// The target or targets, as the Relationship spells them.
    targets: &'a OneOrMany<Uri>,
    /// Whether they are decoded.
    decoding: UriDecoding,
}

impl<'a> DecodedTargets<'a> {
    /// `targets`, written under `decoding`.
    pub(crate) const fn new(targets: &'a OneOrMany<Uri>, decoding: UriDecoding) -> DecodedTargets<'a> {
        DecodedTargets { targets, decoding }
    }
}

impl Serialize for DecodedTargets<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.targets {
            OneOrMany::One(target) => DecodedTarget::new(target, self.decoding).serialize(serializer),
            OneOrMany::Many(targets) => serializer.collect_seq(targets.iter().map(|target| DecodedTarget::new(target, self.decoding))),
        }
    }
}

/// One target, written under a [`UriDecoding`].
#[derive(Debug, Clone, Copy)]
pub(crate) struct DecodedTarget<'a> {
    /// The target.
    target: &'a Uri,
    /// Whether it is decoded.
    decoding: UriDecoding,
}

impl<'a> DecodedTarget<'a> {
    /// `target`, written under `decoding`.
    pub(crate) const fn new(target: &'a Uri, decoding: UriDecoding) -> DecodedTarget<'a> {
        DecodedTarget { target, decoding }
    }
}

impl Serialize for DecodedTarget<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.decoding {
            UriDecoding::Decode => serializer.serialize_str(&decode_percent_encoded_uri(self.target.as_str())),
            UriDecoding::Keep => serializer.serialize_str(self.target.as_str()),
        }
    }
}

/// Decode one percent-encoded URI without altering an undecodable or unescaped value.
///
/// The rule undoes a client's encoding of a whole identifier, such as
/// `urn%3Angsi-ld%3ABuilding%3A1`, so it applies only when what it decodes to
/// is itself a URI (clause 4.6.2). An escape whose decoding is not a URI
/// character -- the `%20` of `urn:ngsi-ld:City:a%20b` -- is part of the
/// identifier as written, and decoding it would name a different entity, or
/// none at all.
///
/// Temporal intent preparation uses the same narrow rule before validating a
/// Relationship target for typed history storage. This keeps current-entity
/// storage unchanged while preventing its durable temporal consequence from
/// assigning different semantics to the same target.
#[must_use]
pub fn decode_percent_encoded_uri(uri: &str) -> Cow<'_, str> {
    if uri.contains('%')
        && let Ok(decoded) = percent_encoding::percent_decode_str(uri).decode_utf8()
        && decoded.parse::<Uri>().is_ok()
    {
        return decoded;
    }
    Cow::Borrowed(uri)
}

#[cfg(test)]
mod tests {
    use crate::uri_decoding::{DecodedTargets, UriDecoding, decode_percent_encoded_uri};
    use andromeda_ngsi_ld_lexicon::uri::Uri;
    use andromeda_ngsi_ld_model::attribute::one_or_many::OneOrMany;
    use serde_json::json;
    use std::borrow::Cow;

    fn uri(text: &str) -> Uri {
        text.parse().expect("a URI")
    }

    #[test]
    fn single_uri_decoding_borrows_plain_input_and_owns_decoded_input() {
        assert!(matches!(decode_percent_encoded_uri("urn:ngsi-ld:Building:1"), Cow::Borrowed(_)));
        pretty_assertions::assert_eq!(decode_percent_encoded_uri("urn%3Angsi-ld%3ABuilding%3A1"), "urn:ngsi-ld:Building:1");
    }

    /// Decoding `%20` would turn a valid identifier into a string no URI
    /// grammar admits, so the escape is kept as the client wrote it.
    #[test]
    fn an_escape_that_decodes_to_a_non_uri_character_is_kept() {
        for raw in ["urn:ngsi-ld:City:a%20b", "https://example.com/a%20b", "urn:ngsi-ld:City:a%0Ab"] {
            assert!(matches!(decode_percent_encoded_uri(raw), Cow::Borrowed(kept) if kept == raw), "{raw}");
        }
    }

    /// A target that is itself percent-encoded once more is decoded once, not
    /// down to its fixpoint.
    #[test]
    fn a_target_is_decoded_once() {
        pretty_assertions::assert_eq!(decode_percent_encoded_uri("urn:b:x%253A1"), "urn:b:x%3A1");
    }

    #[test]
    fn the_typed_targets_are_written_decoded_or_as_stored() {
        let many = OneOrMany::Many(vec![uri("urn:ngsi-ld:City:x%3A1"), uri("urn:ngsi-ld:City:a%20b")]);
        let decoded = serde_json::to_value(DecodedTargets::new(&many, UriDecoding::Decode)).expect("serialises");
        pretty_assertions::assert_eq!(decoded, json!(["urn:ngsi-ld:City:x:1", "urn:ngsi-ld:City:a%20b"]));
        let kept = serde_json::to_value(DecodedTargets::new(&many, UriDecoding::Keep)).expect("serialises");
        pretty_assertions::assert_eq!(kept, json!(["urn:ngsi-ld:City:x%3A1", "urn:ngsi-ld:City:a%20b"]));
    }

    #[test]
    fn one_target_is_written_as_a_string_and_many_as_an_array() {
        let one = OneOrMany::One(uri("urn:ngsi-ld:City:x%3A1"));
        let written = serde_json::to_value(DecodedTargets::new(&one, UriDecoding::Decode)).expect("serialises");
        pretty_assertions::assert_eq!(written, json!("urn:ngsi-ld:City:x:1"));
    }
}
