//! Presentation logic for context source registration (CSR) responses.
//!
//! Applies, over a registration, the one member of Table 6.8.3.2-1 that shapes
//! the representation rather than narrows the query: `sysAttrs`. The clause
//! 4.21 `pick` and `omit` projection is not applied: a registration is not an
//! Entity, and clause 5.2.9 makes its `type`, `endpoint` and `information`
//! mandatory, so the answer always carries the whole registration. The clause
//! 4.5.16 `geometryProperty` selection is not applied either: a registration has
//! no `GeoJSON` representation (clause 6.3.4). Nor is the clause 4.15 `lang`
//! filter: it converts the `LanguageProperty` Attributes of Entities, Table
//! 5.2.9-1 defines no `LanguageProperty` member, and clause 5.10.2.5 answers
//! with the registrations as clause 5.2.9 defines them, so a member a client
//! registered in that shape is written as it was registered.

use crate::error::{RenderError, RenderedShape};
use andromeda_json_literal::piece::JsonPiece;
use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;
use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
use serde::{
    Serialize,
    Serializer,
    ser::{Error as SerializeError, SerializeMap},
};
use serde_json::value::{RawValue, to_raw_value};
use thiserror::Error;

/// The clause 4.8 system timestamps `sysAttrs` governs.
fn is_system_timestamp(key: &str) -> bool {
    key == "createdAt" || key == "modifiedAt"
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
    /// Whether the clause 4.8 system timestamps survive.
    sys_attrs: SysAttrsInclusion,
}

impl<'a> RegistrationRenderer<'a> {
    /// Create a renderer for the given registration.
    #[must_use]
    pub const fn new(registration: &'a ContextSourceRegistration) -> RegistrationRenderer<'a> {
        RegistrationRenderer {
            registration,
            sys_attrs: SysAttrsInclusion::Strip,
        }
    }

    /// Set whether system attributes are included in the response.
    #[must_use]
    pub const fn with_sys_attrs(mut self, sys_attrs: SysAttrsInclusion) -> Self {
        self.sys_attrs = sys_attrs;
        self
    }

    /// Write the registration and keep what strips its members when the
    /// response is written.
    ///
    /// # Errors
    ///
    /// Returns [`RenderError::Serialization`] when the registration cannot be
    /// written, which no registration the broker holds raises.
    pub fn render(self) -> Result<RenderedRegistration, RenderError> {
        let written = to_raw_value(self.registration).map_err(|source| RenderError::serialization(RenderedShape::Resource, source))?;
        Ok(RenderedRegistration {
            written,
            sys_attrs: self.sys_attrs,
        })
    }
}

/// A registration as a response writes it: the members the request's
/// `sysAttrs` leaves, in the order the registration writes them.
#[derive(Debug)]
pub struct RenderedRegistration {
    /// The registration, every member of it written.
    written: Box<RawValue>,
    /// Whether the clause 4.8 system timestamps survive.
    sys_attrs: SysAttrsInclusion,
}

impl RenderedRegistration {
    /// Whether the member `name` is part of the response.
    ///
    /// Clause 4.8: `sysAttrs` governs only the system timestamps.
    fn keeps(&self, name: &str) -> bool {
        self.sys_attrs == SysAttrsInclusion::Include || !is_system_timestamp(name)
    }
}

/// A registration whose written form is no JSON object, which its own
/// serializer never produces.
#[derive(Debug, Error)]
#[error("a registration is written as a JSON object")]
struct RegistrationNotAnObject;

impl Serialize for RenderedRegistration {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let members = JsonPiece::new(&self.written)
            .members()
            .ok_or_else(|| S::Error::custom(RegistrationNotAnObject))?;
        let mut object = serializer.serialize_map(None)?;
        for (name, value) in members.into_iter().filter(|(name, _)| self.keeps(name)) {
            object.serialize_entry(&*name, value.raw())?;
        }
        object.end()
    }
}

#[cfg(test)]
mod tests {
    use crate::registration::RegistrationRenderer;
    use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;
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
    fn the_rendering_carries_every_mandatory_member_of_the_registration() {
        let registration = registration();
        let rendered = document_under(RegistrationRenderer::new(&registration));
        assert_eq!(rendered["id"], "urn:ngsi-ld:ContextSourceRegistration:1");
        assert_eq!(rendered["type"], "ContextSourceRegistration");
        assert_eq!(rendered["endpoint"], "http://source:1026/");
        assert_eq!(rendered["information"], json!([{"entities": [{"type": "Vehicle"}]}]));
    }

    /// Table 5.2.9-1 defines no `LanguageProperty` member, so a member
    /// registered in that shape is written with its whole `languageMap`.
    #[test]
    fn a_language_property_shaped_member_is_written_as_registered() {
        let registration = registration();
        let rendered = document_under(RegistrationRenderer::new(&registration));
        assert_eq!(
            rendered["label"],
            json!({"type": "LanguageProperty", "languageMap": {"en": "Fleet", "fr": "Flotte"}})
        );
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
