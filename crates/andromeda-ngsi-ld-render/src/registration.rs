//! Presentation logic for context source registration (CSR) responses.
//!
//! Applies, over a rendered registration document, the members of Table
//! 6.8.3.2-1 that shape the representation rather than narrow the query:
//! `sysAttrs`, the clause 4.21 `pick`/`omit` member projection, and the
//! clause 4.15 `lang` language negotiation. The clause 4.5.16 `geometryProperty`
//! selection is not applied here: it belongs to the `GeoJSON` envelope the
//! response pipeline builds, and is threaded to that pipeline instead.

use crate::language_negotiation::render_attribute_value;
use andromeda_http_headers::language::LanguageFilter;
use andromeda_ngsi_ld_model::attribute::instance::AttributeValue;
use andromeda_ngsi_ld_query::projection::grammar::RawProjectionTerm;
use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
use serde_json::{Map, Value};
use std::collections::HashSet;

/// The structural members a registration always keeps, whatever `pick`/`omit`
/// name: `id` and `type` identify the document (clause 5.2.9 cardinality 1),
/// and `@context` is a JSON-LD transport concern, never a structural member.
fn is_always_retained(key: &str) -> bool {
    key == "id" || key == "type" || key == "@context"
}

/// Whether a rendered member is a language map the clause 4.15 `lang`
/// negotiation applies to.
fn is_language_shaped(value: &Value) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    object.contains_key("languageMap") || object.get("type").and_then(Value::as_str) == Some("LanguageProperty")
}

/// Renders a context source registration payload for HTTP response.
///
/// Strips the clause 4.8 system timestamps (`createdAt`, `modifiedAt`) unless
/// `sysAttrs` was requested. The registration status members (`status`,
/// `timesSent`, `timesFailed`, `lastSuccess`, `lastFailure`) are read-only
/// auto-generated members (clause 5.2.9 Table 5.2.9-2) returned when known;
/// `sysAttrs` does not govern them, so they are never stripped.
#[derive(Debug)]
pub struct RegistrationRenderer<'a> {
    /// The registration payload to scrub.
    payload: Value,
    /// Whether the clause 4.8 system attributes survive.
    sys_attrs: SysAttrsInclusion,
    /// The clause 4.21 `pick` terms, when the request narrowed the members.
    pick: Option<&'a [RawProjectionTerm]>,
    /// The clause 4.21 `omit` terms, when the request dropped members.
    omit: Option<&'a [RawProjectionTerm]>,
    /// The clause 4.15 `lang` preferences, when the request negotiated language.
    language_filter: Option<&'a LanguageFilter>,
}

impl<'a> RegistrationRenderer<'a> {
    /// Create a renderer for the given payload.
    #[must_use]
    pub const fn new(payload: Value) -> RegistrationRenderer<'a> {
        RegistrationRenderer {
            payload,
            sys_attrs: SysAttrsInclusion::Strip,
            pick: None,
            omit: None,
            language_filter: None,
        }
    }

    /// Set whether system attributes are included in the response.
    #[must_use]
    pub const fn with_sys_attrs(mut self, sys_attrs: SysAttrsInclusion) -> Self {
        self.sys_attrs = sys_attrs;
        self
    }

    /// Set the clause 4.21 `pick` terms the response is narrowed to.
    #[must_use]
    pub const fn with_pick(mut self, pick: Option<&'a [RawProjectionTerm]>) -> Self {
        self.pick = pick;
        self
    }

    /// Set the clause 4.21 `omit` terms the response drops.
    #[must_use]
    pub const fn with_omit(mut self, omit: Option<&'a [RawProjectionTerm]>) -> Self {
        self.omit = omit;
        self
    }

    /// Set the clause 4.15 `lang` preferences applied to language maps.
    #[must_use]
    pub const fn with_language_filter(mut self, language_filter: Option<&'a LanguageFilter>) -> Self {
        self.language_filter = language_filter;
        self
    }

    /// Apply the clause 4.21 `pick`/`omit` member projection to the document.
    ///
    /// The two are mutually exclusive (clause 4.21); each is applied on its own
    /// so a caller that supplies both still gets a well-defined result. Member
    /// names are matched by the compact term the client wrote, which the
    /// rendered document is keyed by under the same negotiated `@context`.
    /// `id` and `type` are retained whatever the projection names, since a
    /// registration document without them is not one.
    fn apply_projection(map: &mut Map<String, Value>, pick: Option<&[RawProjectionTerm]>, omit: Option<&[RawProjectionTerm]>) {
        if let Some(pick) = pick.filter(|terms| !terms.is_empty()) {
            let kept: HashSet<&str> = pick.iter().map(|term| term.name.as_str()).collect();
            map.retain(|key, _| is_always_retained(key) || kept.contains(key.as_str()));
        }
        if let Some(omit) = omit.filter(|terms| !terms.is_empty()) {
            let dropped: HashSet<&str> = omit.iter().map(|term| term.name.as_str()).collect();
            map.retain(|key, _| is_always_retained(key) || !dropped.contains(key.as_str()));
        }
    }

    /// Reduce every language-map member to the clause 4.15 negotiated `Property`.
    fn apply_language_negotiation(map: &mut Map<String, Value>, filter: &LanguageFilter) {
        for value in map.values_mut() {
            if !is_language_shaped(value) {
                continue;
            }
            // Clone required: `AttributeValue` deserialisation consumes the JSON,
            // and only a member that is actually a language map reaches here.
            let Ok(attribute) = serde_json::from_value::<AttributeValue>(value.clone()) else {
                continue;
            };
            if let Ok(negotiated) = render_attribute_value(&attribute, Some(filter)) {
                *value = negotiated;
            }
        }
    }

    /// Strip the members clause 4.8 withholds, project and negotiate, and return
    /// the payload.
    ///
    /// Removing members and negotiating language cannot fail in a way a caller
    /// could act on, so this answers with the value rather than a `Result`.
    #[must_use]
    pub fn render(self) -> Value {
        let RegistrationRenderer {
            mut payload,
            sys_attrs,
            pick,
            omit,
            language_filter,
        } = self;
        let Some(obj) = payload.as_object_mut() else {
            return payload;
        };
        // Clause 4.8: `sysAttrs` governs only the system timestamps. The
        // registration status members are read-only auto-generated members
        // (clause 5.2.9 Table 5.2.9-2) returned when known regardless of
        // `sysAttrs`, so they are not stripped here.
        if sys_attrs == SysAttrsInclusion::Strip {
            obj.remove("createdAt");
            obj.remove("modifiedAt");
        }
        Self::apply_projection(obj, pick, omit);
        if let Some(filter) = language_filter {
            Self::apply_language_negotiation(obj, filter);
        }
        payload
    }
}

#[cfg(test)]
mod tests {
    use crate::registration::RegistrationRenderer;
    use andromeda_http_headers::language::LanguageFilter;
    use andromeda_ngsi_ld_query::projection::grammar::parse_projection_list;
    use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
    use serde_json::json;

    fn registration() -> serde_json::Value {
        json!({
            "id": "urn:ngsi-ld:ContextSourceRegistration:1",
            "type": "ContextSourceRegistration",
            "endpoint": "http://source:1026",
            "information": [{"entities": [{"type": "Vehicle"}]}],
            "createdAt": "2026-01-01T00:00:00Z",
            "modifiedAt": "2026-01-02T00:00:00Z"
        })
    }

    #[test]
    fn sys_attrs_are_stripped_by_default_and_kept_when_requested() {
        let stripped = RegistrationRenderer::new(registration()).render();
        assert!(stripped.get("createdAt").is_none());
        assert!(stripped.get("modifiedAt").is_none());

        let kept = RegistrationRenderer::new(registration()).with_sys_attrs(SysAttrsInclusion::Include).render();
        assert_eq!(kept["createdAt"], "2026-01-01T00:00:00Z");
    }

    #[test]
    fn pick_narrows_to_the_named_members_and_keeps_id_and_type() {
        let pick = parse_projection_list("endpoint").expect("valid pick");
        let rendered = RegistrationRenderer::new(registration()).with_pick(Some(&pick)).render();
        assert_eq!(rendered["id"], "urn:ngsi-ld:ContextSourceRegistration:1");
        assert_eq!(rendered["type"], "ContextSourceRegistration");
        assert_eq!(rendered["endpoint"], "http://source:1026");
        assert!(rendered.get("information").is_none(), "unpicked member survived: {rendered}");
    }

    #[test]
    fn omit_drops_the_named_members_but_never_id_or_type() {
        let omit = parse_projection_list("endpoint,type").expect("valid omit");
        let rendered = RegistrationRenderer::new(registration()).with_omit(Some(&omit)).render();
        assert!(rendered.get("endpoint").is_none(), "omitted member survived: {rendered}");
        assert_eq!(rendered["id"], "urn:ngsi-ld:ContextSourceRegistration:1");
        assert_eq!(rendered["type"], "ContextSourceRegistration", "type must survive omit");
        assert_eq!(rendered["information"], json!([{"entities": [{"type": "Vehicle"}]}]));
    }

    #[test]
    fn lang_reduces_a_language_map_member_to_the_preferred_language() {
        let mut document = registration();
        document.as_object_mut().expect("object").insert(
            "label".to_string(),
            json!({"type": "LanguageProperty", "languageMap": {"en": "Fleet", "fr": "Flotte"}}),
        );
        let filter = LanguageFilter::parse("fr").expect("valid lang filter");
        let rendered = RegistrationRenderer::new(document).with_language_filter(Some(&filter)).render();
        assert_eq!(rendered["label"]["type"], "Property");
        assert_eq!(rendered["label"]["value"], "Flotte");
        assert_eq!(rendered["label"]["lang"], "fr");
    }

    #[test]
    fn a_non_language_member_is_left_untouched_by_lang() {
        let filter = LanguageFilter::parse("fr").expect("valid lang filter");
        let rendered = RegistrationRenderer::new(registration()).with_language_filter(Some(&filter)).render();
        assert_eq!(rendered["endpoint"], "http://source:1026");
    }

    #[test]
    fn an_empty_projection_leaves_every_member() {
        let pick = parse_projection_list("").unwrap_or_default();
        let rendered = RegistrationRenderer::new(registration()).with_pick(Some(&pick)).render();
        assert_eq!(rendered["endpoint"], "http://source:1026");
        assert_eq!(rendered["information"], json!([{"entities": [{"type": "Vehicle"}]}]));
    }
}
