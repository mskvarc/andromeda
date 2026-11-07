// SPDX-FileCopyrightText: 2014-2025 Sean McArthur
// SPDX-License-Identifier: MIT

//! A single `link-value` within a `Link` header (RFC 8288 section 3).
//!
//! This file is derived from the disabled `Link` implementation in
//! <https://github.com/hyperium/headers> and includes local modifications.

use crate::{
    link::{error::LinkParseError, quoting::verify_and_trim, relation_type::RelationType},
    media_type::APPLICATION_LD_JSON,
};
use iri_rs::{IriBuf, IriRefBuf};
use mediatype::MediaTypeBuf;
use std::fmt;

/// One `link-value`: a target IRI and the parameters NGSI-LD reads from it.
///
/// Carries `rel` (forward relation types), `type` (media type hint) and
/// `profile` (RFC 8288 section 3.4, clause 6.3.21). Every other RFC 8288
/// parameter is discarded during parsing.
///
/// The target is an [`IriRefBuf`] rather than an absolute IRI because a
/// `link-value` legitimately carries a relative reference: NGSI-LD pagination
/// links (clause 6.3.10) are path-and-query references such as
/// `/entities?offset=20&limit=10`, while `@context` links are absolute. The
/// `profile` is an absolute [`IriBuf`], the URI form RFC 8288 section 3.4
/// mandates for it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LinkValue {
    /// Target IRI reference.
    link: IriRefBuf,
    /// Forward relation types: `rel`.
    rel: Option<Vec<RelationType>>,
    /// Hint on the media type of the result: `type`.
    media_type: Option<MediaTypeBuf>,
    /// Profile URI: `profile`.
    profile: Option<IriBuf>,
}

impl LinkValue {
    /// A link-value naming `link`, with no parameters.
    #[must_use]
    pub const fn new(link: IriRefBuf) -> LinkValue {
        LinkValue {
            link,
            rel: None,
            media_type: None,
            profile: None,
        }
    }

    /// The target IRI reference.
    #[must_use]
    pub const fn link(&self) -> &IriRefBuf {
        &self.link
    }

    /// The `rel` parameter, if present.
    #[must_use]
    pub fn rel(&self) -> Option<&[RelationType]> {
        self.rel.as_deref()
    }

    /// The `profile` parameter, if present.
    #[must_use]
    pub const fn profile(&self) -> Option<&IriBuf> {
        self.profile.as_ref()
    }

    /// The `type` parameter, if present: a hint on the media type of the
    /// target (RFC 8288 section 3.4.1).
    #[must_use]
    pub const fn media_type(&self) -> Option<&MediaTypeBuf> {
        self.media_type.as_ref()
    }

    /// Add a relation type to the `rel` parameter.
    #[must_use]
    pub fn push_rel(mut self, rel: RelationType) -> LinkValue {
        self.rel.get_or_insert_with(Vec::new).push(rel);
        self
    }

    /// Set the `type` parameter.
    #[must_use]
    pub fn set_media_type(mut self, media_type: MediaTypeBuf) -> LinkValue {
        self.media_type = Some(media_type);
        self
    }

    /// Build a link-value with optional `rel` and `type` parameters.
    #[must_use]
    pub fn with_params(target: IriRefBuf, rel: Option<RelationType>, media_type: Option<MediaTypeBuf>) -> LinkValue {
        let mut value = LinkValue::new(target);
        if let Some(rel) = rel {
            value = value.push_rel(rel);
        }
        if let Some(media_type) = media_type {
            value = value.set_media_type(media_type);
        }
        value
    }

    /// A JSON-LD context link-value with the standard relation and media type.
    ///
    /// The context is an absolute IRI: a context link a response or a
    /// forwarded request names is always absolute, so the conversion to the
    /// link target's reference form happens here and nowhere else.
    #[must_use]
    pub fn jsonld_context(context_url: IriBuf) -> LinkValue {
        LinkValue::new(IriRefBuf::from(context_url))
            .push_rel(RelationType::JsonLdContext)
            .set_media_type(MediaTypeBuf::from(&APPLICATION_LD_JSON))
    }

    /// Whether this link-value carries the given relation type.
    #[must_use]
    pub fn has_rel(&self, rel: &RelationType) -> bool {
        self.rel().is_some_and(|rels| rels.contains(rel))
    }

    /// Whether this link-value is a JSON-LD context link.
    #[must_use]
    pub fn is_context_link(&self) -> bool {
        self.rel().is_some_and(|rels| rels.iter().any(RelationType::is_jsonld_context))
    }

    /// Apply one `name=value` parameter segment to this link-value.
    ///
    /// A parameter already set is kept: RFC 8288 section 3.3 says the first
    /// occurrence of `rel` wins and a recipient must ignore the rest, and the
    /// same rule is applied to `type` and `profile` for consistency. A
    /// parameter this broker does not read is ignored outright, per section
    /// 3.4.1.
    ///
    /// # Errors
    ///
    /// Returns [`LinkParseError`] when the segment has no parameter name, when a
    /// recognised parameter has an empty value, when `profile` is unquoted,
    /// when a quoted value is unbalanced, or when `type` is not a media type or
    /// `profile` is not an absolute IRI.
    pub fn apply_param(&mut self, segment: &str) -> Result<(), LinkParseError> {
        let mut parts = segment.splitn(2, '=');
        let name = match parts.next() {
            Some(name) if !name.trim().is_empty() => name.trim(),
            Some(_) | None => return Err(LinkParseError),
        };

        if "rel".eq_ignore_ascii_case(name) {
            if self.rel.is_none() {
                self.rel = Some(parse_relation_types(parts.next())?);
            }
        } else if "type".eq_ignore_ascii_case(name) {
            if self.media_type.is_none() {
                self.media_type = Some(media_type_value(parts.next())?);
            }
        } else if "profile".eq_ignore_ascii_case(name) && self.profile.is_none() {
            self.profile = Some(quoted_value(parts.next())?.parse::<IriBuf>().map_err(|_| LinkParseError)?);
        }

        Ok(())
    }
}

/// The inside of a quoted parameter value.
fn quoted_value(raw: Option<&str>) -> Result<&str, LinkParseError> {
    verify_and_trim(raw.filter(|value| !value.is_empty()).ok_or(LinkParseError)?.trim(), b'"', b'"')
}

/// The media type a `type` parameter names, written bare or quoted.
///
/// RFC 8288 Appendix B.3 reads a parameter value as a quoted string when it
/// opens with a double quote and as the characters up to the next `;` or `,`
/// otherwise, so `type=application/ld+json` names the same media type as
/// `type="application/ld+json"`. A `profile` carries a URI, which a bare
/// value cannot hold, so only `type` takes this reading.
fn media_type_value(raw: Option<&str>) -> Result<MediaTypeBuf, LinkParseError> {
    let raw = raw.map(str::trim).filter(|value| !value.is_empty()).ok_or(LinkParseError)?;
    let written = if raw.starts_with('"') { verify_and_trim(raw, b'"', b'"')? } else { raw };
    written.parse().map_err(|_| LinkParseError)
}

/// Parse the space-separated relation types of a `rel` parameter.
fn parse_relation_types(raw: Option<&str>) -> Result<Vec<RelationType>, LinkParseError> {
    let raw = raw.filter(|value| !value.is_empty()).ok_or(LinkParseError)?;
    raw.trim_matches(|character: char| character == '"' || character.is_whitespace())
        .split(' ')
        .filter(|token| !token.is_empty())
        .map(|token| token.trim().parse::<RelationType>().map_err(|_| LinkParseError))
        .collect::<Result<Vec<_>, _>>()
}

impl fmt::Display for LinkValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "<{}>", self.link)?;

        if let Some(ref rel) = self.rel {
            f.write_str("; rel=\"")?;
            for (index, relation) in rel.iter().enumerate() {
                if index > 0 {
                    f.write_str(" ")?;
                }
                write!(f, "{relation}")?;
            }
            f.write_str("\"")?;
        }
        if let Some(ref media_type) = self.media_type {
            write!(f, "; type=\"{media_type}\"")?;
        }
        if let Some(ref profile) = self.profile {
            write!(f, "; profile=\"{profile}\"")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::link::{relation_type::RelationType, value::LinkValue};
    use iri_rs::{IriBuf, IriRefBuf};
    use mediatype::MediaTypeBuf;

    /// A validated IRI reference, for building link targets in tests.
    fn iri_ref(value: &str) -> IriRefBuf {
        IriRefBuf::new(value).expect("valid IRI reference")
    }

    /// A validated absolute IRI, for building profile values in tests.
    fn iri(value: &str) -> IriBuf {
        IriBuf::new(value).expect("valid IRI")
    }

    #[test]
    fn a_relation_a_link_value_carries_is_reported_and_others_are_not() {
        let value = LinkValue::new(iri_ref("/page")).push_rel(RelationType::Next);
        assert!(value.has_rel(&RelationType::Next));
        assert!(!value.has_rel(&RelationType::Prev));
    }

    #[test]
    fn only_the_json_ld_relation_makes_a_context_link() {
        let context = LinkValue::new(iri_ref("https://example.com/ctx.jsonld")).push_rel(RelationType::JsonLdContext);
        assert!(context.is_context_link());

        let pagination = LinkValue::new(iri_ref("/page/2")).push_rel(RelationType::Next);
        assert!(!pagination.is_context_link());
    }

    #[test]
    fn a_json_ld_context_value_carries_the_standard_relation_and_media_type() {
        let value = LinkValue::jsonld_context(iri("https://example.com/ctx.jsonld"));
        pretty_assertions::assert_eq!(
            value.to_string(),
            r#"<https://example.com/ctx.jsonld>; rel="http://www.w3.org/ns/json-ld#context"; type="application/ld+json""#
        );
    }

    #[test]
    fn a_relative_pagination_reference_is_accepted_as_a_target() {
        // Clause 6.3.10 pagination links are relative references, so the target
        // type must accept them alongside the absolute `@context` URLs.
        let value = LinkValue::new(iri_ref("/entities?offset=20&limit=10")).push_rel(RelationType::Next);
        pretty_assertions::assert_eq!(value.link().as_str(), "/entities?offset=20&limit=10");
    }

    #[test]
    fn every_parameter_set_appears_in_the_rendering() {
        let mut value = LinkValue::new(iri_ref("http://example.com/ctx.jsonld"))
            .push_rel(RelationType::JsonLdContext)
            .set_media_type("application/ld+json".parse::<MediaTypeBuf>().expect("valid media type"));
        value
            .apply_param(r#"profile="https://uri.etsi.org/ngsi-ld/profile/version""#)
            .expect("a profile parameter applies");
        assert!(value.to_string().contains(r#"profile="https://uri.etsi.org/ngsi-ld/profile/version""#));
    }

    #[test]
    fn with_params_applies_only_the_parameters_given() {
        let bare = LinkValue::with_params(iri_ref("/page"), Some(RelationType::Next), None);
        pretty_assertions::assert_eq!(bare.to_string(), r#"</page>; rel="next""#);

        let typed = LinkValue::with_params(
            iri_ref("/page"),
            Some(RelationType::Next),
            Some("application/json".parse::<MediaTypeBuf>().expect("valid media type")),
        );
        pretty_assertions::assert_eq!(typed.to_string(), r#"</page>; rel="next"; type="application/json""#);
    }

    #[test]
    fn a_parameter_already_set_is_not_overwritten() {
        let mut value = LinkValue::new(iri_ref("/page"));
        value.apply_param(r#"rel="next""#).expect("first rel applies");
        value.apply_param(r#"rel="prev""#).expect("second rel is ignored");
        pretty_assertions::assert_eq!(value.rel(), Some([RelationType::Next].as_slice()));
    }

    #[test]
    fn a_parameter_this_broker_does_not_read_is_ignored() {
        let mut value = LinkValue::new(iri_ref("/page"));
        value.apply_param("hreflang=en").expect("unknown parameter is ignored");
        value.apply_param(r#"title="Chapter 2""#).expect("unknown parameter is ignored");
        pretty_assertions::assert_eq!(value.rel(), None);
    }

    #[test]
    fn a_parameter_with_no_name_or_no_value_is_rejected() {
        let mut value = LinkValue::new(iri_ref("/page"));
        assert!(value.apply_param(r#"="next""#).is_err());
        assert!(value.apply_param("rel=").is_err());
        assert!(value.apply_param("profile=").is_err());
    }

    /// A fresh link-value with the parameter segment `raw` applied.
    fn applied(raw: &str) -> LinkValue {
        let mut value = LinkValue::new(iri_ref("https://example.com/ctx.jsonld"));
        value.apply_param(raw).expect("the parameter applies");
        value
    }

    fn ld_json() -> MediaTypeBuf {
        "application/ld+json".parse::<MediaTypeBuf>().expect("valid media type")
    }

    #[test]
    fn an_unquoted_type_is_read_as_its_media_type() {
        pretty_assertions::assert_eq!(applied("type=application/ld+json").media_type(), Some(&ld_json()));
    }

    #[test]
    fn a_quoted_type_is_read_as_its_media_type() {
        pretty_assertions::assert_eq!(applied(r#"type="application/ld+json""#).media_type(), Some(&ld_json()));
    }

    #[test]
    fn an_empty_type_is_rejected_quoted_or_not() {
        let mut value = LinkValue::new(iri_ref("/page"));
        assert!(value.apply_param("type=").is_err());
        assert!(value.apply_param(r#"type="""#).is_err());
    }

    #[test]
    fn a_type_that_is_not_a_media_type_is_rejected_quoted_or_not() {
        let mut value = LinkValue::new(iri_ref("/page"));
        assert!(value.apply_param("type=not a type").is_err());
        assert!(value.apply_param(r#"type="not a type""#).is_err());
        assert!(value.apply_param(r#"type="application/ld+json"#).is_err());
    }

    #[test]
    fn an_unquoted_profile_is_rejected() {
        let mut value = LinkValue::new(iri_ref("/page"));
        assert!(value.apply_param("profile=https://uri.etsi.org/ngsi-ld/profile/1.8").is_err());
    }

    #[test]
    fn a_profile_that_is_not_an_absolute_iri_is_rejected() {
        // RFC 8288 section 3.4 requires the `profile` value to be a URI, so a
        // relative reference is not a usable profile and fails the parameter.
        let mut value = LinkValue::new(iri_ref("/page"));
        assert!(value.apply_param(r#"profile="/relative""#).is_err());

        let mut value = LinkValue::new(iri_ref("/page"));
        value
            .apply_param(r#"profile="https://uri.etsi.org/ngsi-ld/profile/version""#)
            .expect("an absolute profile IRI is accepted");
        pretty_assertions::assert_eq!(value.profile().map(IriBuf::as_str), Some("https://uri.etsi.org/ngsi-ld/profile/version"));
    }
}
