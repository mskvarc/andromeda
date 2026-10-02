//! The value of a `GeoProperty` (clause 4.7).
//!
//! Clause 4.7.1 makes the value a `GeoJSON` geometry, any RFC 7946 geometry but a
//! `GeometryCollection`. Expansion keeps it as a `GeoJSON` vocabulary node whose
//! coordinates are nested `@list`s; clause 4.7.2 also admits the geometry
//! written as a JSON string, which expansion leaves a plain string literal.
//! The first is held as its positions with its `bbox` and foreign members
//! (see [`crate::geometry_value`]), the second as its text together with the
//! geometry it parses into, the text keeping every member verbatim.

use crate::geometry_value::GeometryValue;
use andromeda_ngsi_ld_geometry::{form::GeometryForm, geojson::GeometryError};
use serde_json::Value;

/// A geometry written as a JSON string (clause 4.7.2).
///
/// Admitted only when the text parses into a geometry object, so the parsed
/// form is always available beside the text that is written back.
#[derive(Clone, Debug, PartialEq)]
pub struct EncodedGeometry {
    /// The text as written.
    text: String,
    /// The geometry the text parses into.
    form: GeometryForm,
}

impl EncodedGeometry {
    /// Admit `text` when it is the JSON text of a geometry clause 4.7.1 allows.
    ///
    /// # Errors
    ///
    /// Returns [`GeometryError::EncodedNotJson`] when `text` is not JSON, and
    /// the refusal of the compact `GeoJSON` reading otherwise.
    pub fn parse(text: String) -> Result<EncodedGeometry, GeometryError> {
        let value: Value = serde_json::from_str(&text).map_err(GeometryError::EncodedNotJson)?;
        let form = GeometryForm::from_compact_value(&value)?;
        Ok(EncodedGeometry { text, form })
    }

    /// The text as written.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// The geometry the text parses into.
    #[must_use]
    pub const fn form(&self) -> &GeometryForm {
        &self.form
    }
}

/// The geometry of one `GeoProperty` instance.
#[derive(Clone, Debug, PartialEq)]
pub enum GeoValue {
    /// The `GeoJSON` vocabulary node, held as its positions and its members.
    Geometry(GeometryValue),
    /// The clause 4.7.2 string form.
    Encoded(EncodedGeometry),
}

impl GeoValue {
    /// The geometry, whichever way it was written.
    #[must_use]
    pub const fn form(&self) -> &GeometryForm {
        match self {
            GeoValue::Geometry(value) => value.form(),
            GeoValue::Encoded(encoded) => encoded.form(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::geo_value::EncodedGeometry;
    use andromeda_ngsi_ld_geometry::{geojson::GeometryError, geometry_type::GeometryType};

    #[test]
    fn a_string_geometry_is_kept_as_its_text_and_read_as_its_geometry() {
        let encoded = EncodedGeometry::parse(r#"{"type":"Point","coordinates":[1,2]}"#.to_owned()).expect("a point");
        pretty_assertions::assert_eq!(encoded.as_str(), r#"{"type":"Point","coordinates":[1,2]}"#);
        pretty_assertions::assert_eq!(encoded.form().geometry_type(), GeometryType::Point);
    }

    #[test]
    fn a_string_that_is_no_geometry_is_refused() {
        assert!(matches!(EncodedGeometry::parse("POINT(1 2)".to_owned()), Err(GeometryError::EncodedNotJson(_))));
        assert!(matches!(EncodedGeometry::parse("[1, 2]".to_owned()), Err(GeometryError::NotAnObject)));
        assert!(matches!(
            EncodedGeometry::parse(r#"{"type":"GeometryCollection","geometries":[]}"#.to_owned()),
            Err(GeometryError::GeometryCollectionUnsupported)
        ));
    }
}
