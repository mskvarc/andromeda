//! A single `link-value` within a `Link` header (RFC 8288 section 3).

use crate::link::{error::LinkParseError, quoting::verify_and_trim, relation_type::RelationType};
use mediatype::MediaTypeBuf;
use std::{borrow::Cow, fmt};

/// One `link-value`: a target IRI and the parameters NGSI-LD reads from it.
///
/// Carries `rel` (forward relation types), `type` (media type hint) and
/// `profile` (RFC 8288 section 3.4, clause 6.3.21). Every other RFC 8288
/// parameter is discarded during parsing.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LinkValue {
    /// Target IRI.
    link: Cow<'static, str>,
    /// Forward relation types: `rel`.
    rel: Option<Vec<RelationType>>,
    /// Hint on the media type of the result: `type`.
    media_type: Option<MediaTypeBuf>,
    /// Profile URI: `profile`.
    profile: Option<Cow<'static, str>>,
}

impl LinkValue {
    /// `application/ld+json`, the media type of a JSON-LD context link.
    pub const JSON_LD_MEDIA_TYPE: &str = "application/ld+json";

    /// A link-value naming `uri`, with no parameters.
    pub fn new<T>(uri: T) -> LinkValue
    where
        T: Into<Cow<'static, str>>,
    {
        LinkValue {
            link: uri.into(),
            rel: None,
            media_type: None,
            profile: None,
        }
    }

    /// The target IRI.
    pub fn link(&self) -> &str {
        self.link.as_ref()
    }

    /// The `rel` parameter, if present.
    pub fn rel(&self) -> Option<&[RelationType]> {
        self.rel.as_deref()
    }

    /// The `profile` parameter, if present.
    pub fn profile(&self) -> Option<&str> {
        self.profile.as_deref()
    }

    /// Add a relation type to the `rel` parameter.
    pub fn push_rel(mut self, rel: RelationType) -> LinkValue {
        self.rel.get_or_insert_with(Vec::new).push(rel);
        self
    }

    /// Set the `type` parameter.
    pub fn set_media_type(mut self, media_type: MediaTypeBuf) -> LinkValue {
        self.media_type = Some(media_type);
        self
    }

    /// Set the `profile` parameter (RFC 8288 section 3.4, clause 6.3.21).
    pub fn set_profile<T: Into<Cow<'static, str>>>(mut self, profile: T) -> LinkValue {
        self.profile = Some(profile.into());
        self
    }

    /// Build a link-value with optional `rel` and `type` parameters.
    ///
    /// A `media_type` that does not parse is dropped rather than reported: the
    /// `type` parameter is a hint, and RFC 8288 section 3.4.1 has a recipient
    /// ignore a parameter it cannot use.
    pub fn with_params(target: &str, rel: Option<&str>, media_type: Option<&str>) -> LinkValue {
        let mut value = LinkValue::new(target.to_owned());
        if let Some(rel) = rel {
            match rel.parse::<RelationType>() {
                Ok(rel) => value = value.push_rel(rel),
                Err(infallible) => match infallible {},
            }
        }
        if let Some(media_type) = media_type
            && let Ok(parsed) = media_type.parse::<MediaTypeBuf>()
        {
            value = value.set_media_type(parsed);
        }
        value
    }

    /// A JSON-LD context link-value with the standard relation and media type.
    pub fn jsonld_context(target: &str) -> LinkValue {
        let mut value = LinkValue::new(target.to_owned()).push_rel(RelationType::jsonld_context());
        if let Ok(media_type) = Self::JSON_LD_MEDIA_TYPE.parse::<MediaTypeBuf>() {
            value = value.set_media_type(media_type);
        }
        value
    }

    /// Whether this link-value carries the given relation type.
    pub fn has_rel(&self, rel: &RelationType) -> bool {
        self.rel().is_some_and(|rels| rels.contains(rel))
    }

    /// Whether this link-value is a JSON-LD context link.
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
    /// Returns [`LinkParseError`] when the segment has no parameter name, or
    /// when a recognised parameter has an empty or unquotable value.
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
                self.media_type = Some(quoted_value(parts.next())?.parse().map_err(|_| LinkParseError)?);
            }
        } else if "profile".eq_ignore_ascii_case(name) && self.profile.is_none() {
            self.profile = Some(Cow::Owned(quoted_value(parts.next())?.to_owned()));
        }

        Ok(())
    }
}

/// The inside of a quoted parameter value.
fn quoted_value(raw: Option<&str>) -> Result<&str, LinkParseError> {
    verify_and_trim(raw.filter(|value| !value.is_empty()).ok_or(LinkParseError)?.trim(), b'"', b'"')
}

/// Parse the space-separated relation types of a `rel` parameter.
fn parse_relation_types(raw: Option<&str>) -> Result<Vec<RelationType>, LinkParseError> {
    let raw = raw.filter(|value| !value.is_empty()).ok_or(LinkParseError)?;
    Ok(raw
        .trim_matches(|character: char| character == '"' || character.is_whitespace())
        .split(' ')
        .filter(|token| !token.is_empty())
        .map(|token| match token.trim().parse::<RelationType>() {
            Ok(rel) => rel,
            Err(infallible) => match infallible {},
        })
        .collect())
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
    use mediatype::MediaTypeBuf;

    #[test]
    fn a_relation_a_link_value_carries_is_reported_and_others_are_not() {
        let value = LinkValue::new("/page").push_rel(RelationType::Next);
        assert!(value.has_rel(&RelationType::Next));
        assert!(!value.has_rel(&RelationType::Prev));
    }

    #[test]
    fn only_the_json_ld_relation_makes_a_context_link() {
        let context = LinkValue::new("https://example.com/ctx.jsonld").push_rel(RelationType::jsonld_context());
        assert!(context.is_context_link());

        let pagination = LinkValue::new("/page/2").push_rel(RelationType::Next);
        assert!(!pagination.is_context_link());
    }

    #[test]
    fn a_json_ld_context_value_carries_the_standard_relation_and_media_type() {
        let value = LinkValue::jsonld_context("https://example.com/ctx.jsonld");
        pretty_assertions::assert_eq!(
            value.to_string(),
            r#"<https://example.com/ctx.jsonld>; rel="http://www.w3.org/ns/json-ld#context"; type="application/ld+json""#
        );
    }

    #[test]
    fn every_parameter_set_appears_in_the_rendering() {
        let value = LinkValue::new("http://example.com/ctx.jsonld")
            .push_rel(RelationType::jsonld_context())
            .set_media_type("application/ld+json".parse::<MediaTypeBuf>().expect("valid media type"))
            .set_profile("https://uri.etsi.org/ngsi-ld/profile/version");
        assert!(value.to_string().contains(r#"profile="https://uri.etsi.org/ngsi-ld/profile/version""#));
    }

    #[test]
    fn a_parameter_already_set_is_not_overwritten() {
        let mut value = LinkValue::new("/page");
        value.apply_param(r#"rel="next""#).expect("first rel applies");
        value.apply_param(r#"rel="prev""#).expect("second rel is ignored");
        pretty_assertions::assert_eq!(value.rel(), Some([RelationType::Next].as_slice()));
    }

    #[test]
    fn a_parameter_this_broker_does_not_read_is_ignored() {
        let mut value = LinkValue::new("/page");
        value.apply_param("hreflang=en").expect("unknown parameter is ignored");
        value.apply_param(r#"title="Chapter 2""#).expect("unknown parameter is ignored");
        pretty_assertions::assert_eq!(value.rel(), None);
    }

    #[test]
    fn a_parameter_with_no_name_or_no_value_is_rejected() {
        let mut value = LinkValue::new("/page");
        assert!(value.apply_param(r#"="next""#).is_err());
        assert!(value.apply_param("rel=").is_err());
        assert!(value.apply_param("profile=").is_err());
    }

    #[test]
    fn a_media_type_hint_that_does_not_parse_is_dropped_rather_than_reported() {
        let value = LinkValue::with_params("/page", Some("next"), Some("not a media type"));
        pretty_assertions::assert_eq!(value.to_string(), r#"</page>; rel="next""#);
    }
}
