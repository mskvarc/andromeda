//! The four expanded-form IRI keys the fastpath writes that no eight-way
//! table already owns.
//!
//! Held as `String`s rather than read from the vocabulary's `&'static Iri`
//! constants at each use: cloning one of these reuses a known length and
//! capacity, where `Iri::as_str().to_owned()` would dereference the IRI and
//! recompute its UTF-8 length once per entity. The attribute-type keys are
//! not here — those are
//! [`AttributeType`](andromeda_ngsi_ld_lexicon::attribute_type::AttributeType)'s
//! tables, and duplicating them would be a second reading of clause 5.2.4.

use andromeda_ngsi_ld_vocabulary::expanded::{classes, properties};
use std::sync::LazyLock;

/// `DateTime`, expanded. Written for every coerced timestamp.
pub(crate) static DATE_TIME: LazyLock<String> = LazyLock::new(|| classes::DATE_TIME.as_str().to_owned());
/// `scope`, expanded. Written once per entity that carries one.
pub(crate) static SCOPE: LazyLock<String> = LazyLock::new(|| properties::SCOPE.as_str().to_owned());
/// `coordinates`, expanded. Written for every GeoProperty.
pub(crate) static COORDINATES: LazyLock<String> = LazyLock::new(|| properties::COORDINATES.as_str().to_owned());
/// `bbox`, expanded. Written for every GeoProperty that declares one.
pub(crate) static BBOX: LazyLock<String> = LazyLock::new(|| properties::BBOX.as_str().to_owned());
