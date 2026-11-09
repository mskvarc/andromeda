//! Presentation logic for context source registration (CSR) responses.
//!
//! Applies, over a registration, the members of Table 6.8.3.2-1 that shape the
//! representation rather than narrow the query: `sysAttrs`, the clause 4.21
//! `pick`/`omit` member projection, and the clause 4.15 `lang` language
//! negotiation. The clause 4.5.16 `geometryProperty` selection is not applied
//! here: a registration has no `GeoJSON` representation (clause 6.3.4).

use crate::{
    error::{RenderError, RenderedShape},
    language_negotiation::{NegotiatedProperty, negotiated_entry},
};
use andromeda_http_headers::language::LanguageFilter;
use andromeda_json_literal::piece::{JsonKind, JsonPiece};
use andromeda_ngsi_ld_model::{
    attribute::instance::{Attribute, AttributeValue},
    registration::ContextSourceRegistration,
};
use andromeda_ngsi_ld_query::projection::grammar::RawProjectionTerm;
use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
use serde::{
    Serialize,
    Serializer,
    ser::{Error as SerializeError, SerializeMap},
};
use serde_json::value::{RawValue, to_raw_value};
use thiserror::Error;

/// The structural members a registration always keeps, whatever `pick`/`omit`
/// name: `id` and `type` identify the document (clause 5.2.9 cardinality 1),
/// and `@context` is a JSON-LD transport concern, never a structural member.
fn is_always_retained(key: &str) -> bool {
    key == "id" || key == "type" || key == "@context"
}

/// The clause 4.8 system timestamps `sysAttrs` governs.
fn is_system_timestamp(key: &str) -> bool {
    key == "createdAt" || key == "modifiedAt"
}

/// Whether a written member is a language map the clause 4.15 `lang`
/// negotiation applies to.
fn is_language_shaped(member: JsonPiece<'_>) -> bool {
    member.kind() == JsonKind::Object
        && (member.member("languageMap").is_some()
            || member
                .member("type")
                .and_then(JsonPiece::as_str)
                .is_some_and(|named| named == "LanguageProperty"))
}

/// Renders a context source registration for an HTTP response.
///
/// Strips the clause 4.8 system timestamps (`createdAt`, `modifiedAt`) unless
/// `sysAttrs` was requested. The registration status members (`status`,
/// `timesSent`, `timesFailed`, `lastSuccess`, `lastFailure`) are read-only
/// auto-generated members (clause 5.2.9 Table 5.2.9-2) returned when known;
/// `sysAttrs` does not govern them, so they are never stripped.
#[derive(Debug)]
pub struct RegistrationRenderer<'a> {
    /// The registration to render.
    registration: &'a ContextSourceRegistration,
    /// What the request leaves of the registration's members.
    selection: MemberSelection<'a>,
}

/// What a request's `sysAttrs`, `pick`, `omit` and `lang` leave of the members
/// of a registration.
#[derive(Debug)]
struct MemberSelection<'a> {
    /// Whether the clause 4.8 system attributes survive.
    sys_attrs: SysAttrsInclusion,
    /// The clause 4.21 `pick` terms, when the request narrowed the members.
    pick: Option<&'a [RawProjectionTerm]>,
    /// The clause 4.21 `omit` terms, when the request dropped members.
    omit: Option<&'a [RawProjectionTerm]>,
    /// The clause 4.15 `lang` preferences, when the request negotiated language.
    language_filter: Option<&'a LanguageFilter>,
}

impl MemberSelection<'_> {
    /// Whether the member `name` is part of the response.
    ///
    /// Clause 4.8: `sysAttrs` governs only the system timestamps. Clause 4.21:
    /// a member is kept when `pick` is empty or names it and `omit` does not. Member
    /// names are matched by the compact term the client wrote, which the
    /// rendered registration is keyed by under the same negotiated `@context`.
    /// `id` and `type` are retained whatever the projection names, since a
    /// registration without them is not one.
    fn keeps(&self, name: &str) -> bool {
        if self.sys_attrs == SysAttrsInclusion::Strip && is_system_timestamp(name) {
            return false;
        }
        if is_always_retained(name) {
            return true;
        }
        let named_in = |terms: &[RawProjectionTerm]| terms.iter().any(|term| term.name.as_str() == name);
        let picked = self.pick.filter(|terms| !terms.is_empty()).is_none_or(named_in);
        let omitted = self.omit.filter(|terms| !terms.is_empty()).is_some_and(named_in);
        picked && !omitted
    }
}

impl<'a> RegistrationRenderer<'a> {
    /// Create a renderer for the given registration.
    #[must_use]
    pub const fn new(registration: &'a ContextSourceRegistration) -> RegistrationRenderer<'a> {
        RegistrationRenderer {
            registration,
            selection: MemberSelection {
                sys_attrs: SysAttrsInclusion::Strip,
                pick: None,
                omit: None,
                language_filter: None,
            },
        }
    }

    /// Set whether system attributes are included in the response.
    #[must_use]
    pub const fn with_sys_attrs(mut self, sys_attrs: SysAttrsInclusion) -> Self {
        self.selection.sys_attrs = sys_attrs;
        self
    }

    /// Set the clause 4.21 `pick` terms the response is narrowed to.
    #[must_use]
    pub const fn with_pick(mut self, pick: Option<&'a [RawProjectionTerm]>) -> Self {
        self.selection.pick = pick;
        self
    }

    /// Set the clause 4.21 `omit` terms the response drops.
    #[must_use]
    pub const fn with_omit(mut self, omit: Option<&'a [RawProjectionTerm]>) -> Self {
        self.selection.omit = omit;
        self
    }

    /// Set the clause 4.15 `lang` preferences applied to language maps.
    #[must_use]
    pub const fn with_language_filter(mut self, language_filter: Option<&'a LanguageFilter>) -> Self {
        self.selection.language_filter = language_filter;
        self
    }

    /// Write the registration and keep what narrows and negotiates its members
    /// when the response is written.
    ///
    /// # Errors
    ///
    /// Returns [`RenderError::Serialization`] when the registration cannot be
    /// written, which no registration the broker holds raises.
    pub fn render(self) -> Result<RenderedRegistration<'a>, RenderError> {
        let written = to_raw_value(self.registration).map_err(|source| RenderError::serialization(RenderedShape::Resource, source))?;
        Ok(RenderedRegistration {
            written,
            selection: self.selection,
        })
    }
}

/// A registration as a response writes it: the members the request's
/// `sysAttrs`, `pick`, `omit` and `lang` leave, in the order the registration
/// writes them.
#[derive(Debug)]
pub struct RenderedRegistration<'a> {
    /// The registration, every member of it written.
    written: Box<RawValue>,
    /// What narrows and negotiates the members when they are written out.
    selection: MemberSelection<'a>,
}

/// A registration whose written form is no JSON object, which its own
/// serializer never produces.
#[derive(Debug, Error)]
#[error("a registration is written as a JSON object")]
struct RegistrationNotAnObject;

impl Serialize for RenderedRegistration<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let members = JsonPiece::new(&self.written)
            .members()
            .ok_or_else(|| S::Error::custom(RegistrationNotAnObject))?;
        let mut object = serializer.serialize_map(None)?;
        for (name, value) in members.into_iter().filter(|(name, _)| self.selection.keeps(name)) {
            if let Some(filter) = self.selection.language_filter.filter(|_| is_language_shaped(value))
                && let Ok(AttributeValue::Single(attribute)) = serde_json::from_str::<AttributeValue>(value.text())
                && let Attribute::LanguageProperty(language_property) = &*attribute
                && let Some(entry) = negotiated_entry(language_property, filter)
            {
                object.serialize_entry(&*name, &NegotiatedProperty::from_language_property(language_property, entry))?;
            } else {
                object.serialize_entry(&*name, value.raw())?;
            }
        }
        object.end()
    }
}

#[cfg(test)]
mod tests {
    use crate::registration::RegistrationRenderer;
    use andromeda_http_headers::language::LanguageFilter;
    use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;
    use andromeda_ngsi_ld_query::projection::grammar::ProjectionList;
    use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};

    fn registration() -> ContextSourceRegistration {
        serde_json::from_value(json!({
            "id": "urn:ngsi-ld:ContextSourceRegistration:1",
            "type": "ContextSourceRegistration",
            "endpoint": "http://source:1026/",
            "information": [{"entities": [{"type": "Vehicle"}]}],
            "createdAt": "2026-01-01T00:00:00Z",
            "modifiedAt": "2026-01-02T00:00:00Z",
            "label": {"type": "LanguageProperty", "languageMap": {"en": "Fleet", "fr": "Flotte"}}
        }))
        .expect("a registration")
    }

    /// The document the renderer writes for the registration under its options.
    fn document_under(renderer: RegistrationRenderer<'_>) -> Value {
        let text = serde_json::to_string(&renderer.render().expect("the registration renders")).expect("the rendering writes");
        serde_json::from_str(&text).expect("the rendering is JSON")
    }

    #[test]
    fn sys_attrs_are_stripped_by_default_and_kept_when_requested() {
        let registration = registration();
        let stripped = document_under(RegistrationRenderer::new(&registration));
        assert!(stripped.get("createdAt").is_none());
        assert!(stripped.get("modifiedAt").is_none());

        let kept = document_under(RegistrationRenderer::new(&registration).with_sys_attrs(SysAttrsInclusion::Include));
        assert_eq!(kept["createdAt"], "2026-01-01T00:00:00Z");
    }

    #[test]
    fn pick_narrows_to_the_named_members_and_keeps_id_and_type() {
        let registration = registration();
        let pick = Vec::from("endpoint".parse::<ProjectionList>().expect("valid pick"));
        let rendered = document_under(RegistrationRenderer::new(&registration).with_pick(Some(&pick)));
        assert_eq!(rendered["id"], "urn:ngsi-ld:ContextSourceRegistration:1");
        assert_eq!(rendered["type"], "ContextSourceRegistration");
        assert_eq!(rendered["endpoint"], "http://source:1026/");
        assert!(rendered.get("information").is_none(), "unpicked member survived: {rendered}");
    }

    #[test]
    fn omit_drops_the_named_members_but_never_id_or_type() {
        let registration = registration();
        let omit = Vec::from("endpoint,type".parse::<ProjectionList>().expect("valid omit"));
        let rendered = document_under(RegistrationRenderer::new(&registration).with_omit(Some(&omit)));
        assert!(rendered.get("endpoint").is_none(), "omitted member survived: {rendered}");
        assert_eq!(rendered["id"], "urn:ngsi-ld:ContextSourceRegistration:1");
        assert_eq!(rendered["type"], "ContextSourceRegistration", "type must survive omit");
        assert_eq!(rendered["information"], json!([{"entities": [{"type": "Vehicle"}]}]));
    }

    #[test]
    fn lang_reduces_a_language_map_member_to_the_preferred_language() {
        let registration = registration();
        let filter = LanguageFilter::parse("fr").expect("valid lang filter");
        let rendered = document_under(RegistrationRenderer::new(&registration).with_language_filter(Some(&filter)));
        assert_eq!(rendered["label"]["type"], "Property");
        assert_eq!(rendered["label"]["value"], "Flotte");
        assert_eq!(rendered["label"]["lang"], "fr");
    }

    #[test]
    fn a_language_map_is_left_as_written_without_a_lang_filter() {
        let registration = registration();
        let rendered = document_under(RegistrationRenderer::new(&registration));
        assert_eq!(
            rendered["label"],
            json!({"type": "LanguageProperty", "languageMap": {"en": "Fleet", "fr": "Flotte"}})
        );
    }

    #[test]
    fn a_non_language_member_is_left_untouched_by_lang() {
        let registration = registration();
        let filter = LanguageFilter::parse("fr").expect("valid lang filter");
        let rendered = document_under(RegistrationRenderer::new(&registration).with_language_filter(Some(&filter)));
        assert_eq!(rendered["endpoint"], "http://source:1026/");
    }

    #[test]
    fn an_empty_projection_leaves_every_member() {
        let registration = registration();
        let pick = "".parse::<ProjectionList>().map_or_default(Vec::from);
        let rendered = document_under(RegistrationRenderer::new(&registration).with_pick(Some(&pick)));
        assert_eq!(rendered["endpoint"], "http://source:1026/");
        assert_eq!(rendered["information"], json!([{"entities": [{"type": "Vehicle"}]}]));
    }

    /// The members keep the order the registration writes them in, so a body
    /// built from the rendering reads as the stored registration does.
    #[test]
    fn the_members_keep_the_order_the_registration_writes_them_in() {
        let registration = registration();
        let rendered = RegistrationRenderer::new(&registration).render().expect("the registration renders");
        let text = serde_json::to_string(&rendered).expect("the rendering writes");
        assert!(
            text.starts_with(r#"{"id":"urn:ngsi-ld:ContextSourceRegistration:1","type":"ContextSourceRegistration""#),
            "{text}"
        );
    }
}
