//! The `[request_limits]` table: what one request may cost the broker.
//!
//! Every replica of a deployment runs the same values. A client that a load
//! balancer sends to different replicas then meets the same refusals, and a
//! value one replica stored is one every other replica reads back.

use crate::{header_section_limit::HeaderSectionLimit, human_byte_size::HumanByteSize, human_duration::HumanDuration};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::num::NonZeroUsize;
use thiserror::Error;

/// The deepest `request_limits.max_attribute_nesting` may admit: the deepest
/// nesting every stored value reads back at.
pub const MAX_ATTRIBUTE_NESTING_CEILING: usize = 62;

/// The deepest `request_limits.max_q_nesting` may admit: the deepest a `q`
/// filter's parentheses are parsed at, whatever the limit holds.
pub const MAX_Q_NESTING_CEILING: usize = 64;

/// The default most JSON values one request payload may hold: four entities at
/// the default entity budget, or one geometry of the default position budget
/// (three values a position) with ample room around it.
const DEFAULT_MAX_JSON_VALUES: NonZeroUsize = NonZeroUsize::MIN.saturating_add(3_999_999);

/// The default most JSON values one written entity may hold outside its
/// geometries and JSON literals: room for a temporal write that carries an
/// entity's history in one request, such as a smart meter's 5,000 readings of
/// eight attributes.
const DEFAULT_MAX_ENTITY_JSON_VALUES: NonZeroUsize = NonZeroUsize::MIN.saturating_add(999_999);

/// The default most positions one geometry may hold: room for a detailed
/// administrative boundary in one Polygon.
const DEFAULT_MAX_GEOMETRY_POSITIONS: NonZeroUsize = NonZeroUsize::MIN.saturating_add(999_999);

/// The deepest nesting an attribute is admitted to by default, which is also
/// the depth a `q` or `orderBy` attribute path may walk: the one bound keeps
/// every sub-attribute a write can store addressable by a query.
pub const DEFAULT_MAX_ATTRIBUTE_NESTING: NonZeroUsize = NonZeroUsize::MIN.saturating_add(31);

/// The default most comparisons one `q` filter may make: room for a value list
/// of several hundred identifiers or categories beside the filter's other
/// terms.
const DEFAULT_MAX_Q_COMPARISONS: NonZeroUsize = NonZeroUsize::MIN.saturating_add(999);

/// The default deepest a `q` filter's `;` and `|` combinations may nest.
const DEFAULT_MAX_Q_NESTING: NonZeroUsize = NonZeroUsize::MIN.saturating_add(15);

/// What one request may cost the broker.
#[derive(Deserialize, Serialize, JsonSchema, Debug, Clone, Copy, PartialEq, Eq)]
#[schemars(deny_unknown_fields)]
#[schemars(
    description = "What one request may cost the broker. A load balancer can send any request to any replica, so every replica of one deployment runs the same values."
)]
pub struct RequestLimitsConfig {
    /// The longest one request may take, in human-readable form (`"30s"`).
    #[schemars(
        description = "The longest one request may run. A request whose body is still arriving then is answered `408` `RequestTimeout`, and any other request still running is answered `500` `InternalError`."
    )]
    pub timeout: HumanDuration,
    /// The largest request body accepted, in human-readable form
    /// (`"64 MiB"`). A larger body is answered 413.
    #[schemars(description = "The largest request body the broker accepts, counted after decompression. A larger body is answered `413`.")]
    pub max_body: HumanByteSize,
    /// The largest request header section accepted, from `"8 KiB"` to
    /// `"32 KiB"`, on HTTP/1.1, HTTP/2 and HTTP/3 alike: every field, the
    /// request line's method, target and authority included, costs its name,
    /// its value and 32 bytes. A request over it is answered 431. The
    /// management listener applies the same value.
    #[schemars(
        description = "The largest request header section the broker accepts, on HTTP/1.1, HTTP/2 and HTTP/3 alike. Every field, the method, target and authority included, costs its name, its value and 32 bytes. A request over it is answered a bare `431`. The management listener applies the same limit."
    )]
    #[schemars(extend("x-andromeda-replica-consistent" = true))]
    pub max_header_section: HeaderSectionLimit,
    /// The most JSON values one request payload may hold, counted before the
    /// payload is parsed. Every scalar, array and object is one value. A
    /// payload's processing cost follows this count rather than its bytes; a
    /// request over it is answered 413 like one over `max_body`.
    #[schemars(
        description = "The most JSON values one request body may hold, counted before it is parsed. Every object, array, string, number, boolean and `null` counts once, so the count follows the cost of processing the body rather than its bytes. A body over it is answered `413`. Zero is refused."
    )]
    #[schemars(extend("x-andromeda-replica-consistent" = true))]
    pub max_json_values: NonZeroUsize,
    /// The most JSON values one entity may hold outside its geometries and
    /// JSON literals, counted on the document a write expands to and on the
    /// entity it stores; a write over it is answered 413. A write to an entity
    /// already over a lowered limit lands when it does not grow the entity.
    #[schemars(
        description = "The most JSON values one entity may hold outside its geometries and JSON literals, counted on the expanded document and on the entity a write stores, so several writes cannot grow one entity past it. A write over it is answered `413`. After the limit is lowered, an entity already over it can still be shrunk or rewritten at its size, but not grown. Zero is refused."
    )]
    #[schemars(extend("x-andromeda-replica-consistent" = true))]
    pub max_entity_json_values: NonZeroUsize,
    /// The most positions one `GeoProperty` geometry may hold, in a written
    /// entity and in a query, subscription or registration geometry. A typed
    /// entity's geometry is counted before it is expanded. A written geometry
    /// over it is answered 413, a query's 403 `TooComplexQuery`.
    #[schemars(
        description = "The most positions one geometry may hold, in a written entity and in a query, subscription or registration geometry. A written geometry over it is answered `413`, a query's `403` `TooComplexQuery`. Zero is refused."
    )]
    #[schemars(extend("x-andromeda-replica-consistent" = true))]
    pub max_geometry_positions: NonZeroUsize,
    /// The deepest nesting one written attribute may have, counted in levels:
    /// each sub-attribute, each JSON object or list of a value, and each
    /// object or array inside a JSON literal is one. An attribute nesting
    /// deeper is answered 400 `BadRequestData`. At most 62, the deepest every
    /// stored value reads back at.
    #[schemars(
        description = "The deepest one written attribute may nest: each sub-attribute, each JSON object or list of a value, and each object or array inside a JSON literal is one level. An attribute nesting deeper is answered `400` `BadRequestData`. It is also the depth a `q` or `orderBy` path may walk. It may be raised to the deepest every stored value reads back at."
    )]
    #[schemars(extend("x-andromeda-replica-consistent" = true))]
    pub max_attribute_nesting: NonZeroUsize,
    /// The most comparisons one `q` filter may make, in a query, a delete by
    /// query or a subscription: each Query Term is one, and a value list one
    /// per value. A filter over it is answered 403 `TooComplexQuery`.
    #[schemars(
        description = "The most comparisons one `q` filter may make, in an entity or temporal query, a delete by query, or a subscription. Each Query Term is one comparison, and a value list such as `a==1,2,3` one per value. A filter over it is answered `403` `TooComplexQuery`. A stored subscription is not checked again after the limit is lowered. Zero is refused."
    )]
    #[schemars(extend("x-andromeda-replica-consistent" = true))]
    pub max_q_comparisons: NonZeroUsize,
    /// The deepest one `q` filter's `;` and `|` combinations may nest: `a;b`
    /// nests one level, `a;(b|c)` and `a|b;c` two. A filter nesting deeper is
    /// answered 403 `TooComplexQuery`. At most 64, the deepest the parser
    /// admits parentheses at.
    #[schemars(
        description = "How deep the `;` and `|` combinations of one `q` filter may nest, in an entity or temporal query, a delete by query, or a subscription: `a;b` nests one level, `a;(b|c)` and `a|b;c` two, and parentheses around a single Query Term add none. A filter nesting deeper is answered `403` `TooComplexQuery`. It may be raised to the deepest parentheses are parsed at; parentheses written deeper are refused whatever this holds."
    )]
    #[schemars(extend("x-andromeda-replica-consistent" = true))]
    pub max_q_nesting: NonZeroUsize,
}

impl Default for RequestLimitsConfig {
    fn default() -> RequestLimitsConfig {
        RequestLimitsConfig {
            timeout: HumanDuration::from_secs(30),
            max_body: HumanByteSize::mib(64),
            max_header_section: HeaderSectionLimit::MAXIMUM,
            max_json_values: DEFAULT_MAX_JSON_VALUES,
            max_entity_json_values: DEFAULT_MAX_ENTITY_JSON_VALUES,
            max_geometry_positions: DEFAULT_MAX_GEOMETRY_POSITIONS,
            max_attribute_nesting: DEFAULT_MAX_ATTRIBUTE_NESTING,
            max_q_comparisons: DEFAULT_MAX_Q_COMPARISONS,
            max_q_nesting: DEFAULT_MAX_Q_NESTING,
        }
    }
}

/// The body ceiling does not fit the platform's address width.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum RequestLimitsConfigError {
    /// A ceiling beyond the platform address width would admit every body.
    #[error("request_limits.max_body does not fit the platform address width")]
    MaxBodyExceedsAddressWidth,
}

impl RequestLimitsConfig {
    /// The body ceiling in bytes.
    ///
    /// # Errors
    ///
    /// Returns [`RequestLimitsConfigError::MaxBodyExceedsAddressWidth`] when
    /// `max_body` does not fit a `usize`. Saturating it would turn a
    /// protective ceiling into no ceiling.
    pub fn max_body_bytes(&self) -> Result<usize, RequestLimitsConfigError> {
        usize::try_from(self.max_body.as_u64()).map_err(|_| RequestLimitsConfigError::MaxBodyExceedsAddressWidth)
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{header_section_limit::HeaderSectionLimit, schema::request_limits::RequestLimitsConfig};
    use pretty_assertions::assert_eq;

    #[test]
    fn the_defaults_are_valid_and_documented() {
        let limits = RequestLimitsConfig::default();
        assert_eq!(limits.max_body_bytes(), Ok(67_108_864));
        assert_eq!(limits.max_json_values.get(), 4_000_000);
        assert_eq!(limits.max_entity_json_values.get(), 1_000_000);
        assert_eq!(limits.max_geometry_positions.get(), 1_000_000);
        assert_eq!(limits.max_attribute_nesting.get(), 32);
        assert_eq!(limits.max_q_comparisons.get(), 1000);
        assert_eq!(limits.max_q_nesting.get(), 16);
        assert_eq!(limits.max_header_section, HeaderSectionLimit::MAXIMUM);
    }

    #[test]
    fn the_table_parses_human_readable_values_and_refuses_a_zero_count() {
        let rendered = toml::to_string(&RequestLimitsConfig::default()).expect("the defaults render");
        let parsed: RequestLimitsConfig = toml::from_str(&rendered.replace("\"32.0 KiB\"", "\"8 KiB\"")).expect("the defaults parse");
        assert_eq!(parsed.max_header_section, HeaderSectionLimit::MINIMUM);
        let zero = rendered.replace("max_json_values = 4000000", "max_json_values = 0");
        assert!(toml::from_str::<RequestLimitsConfig>(&zero).is_err());
        for zero in [
            rendered.replace("max_q_comparisons = 1000", "max_q_comparisons = 0"),
            rendered.replace("max_q_nesting = 16", "max_q_nesting = 0"),
        ] {
            assert!(toml::from_str::<RequestLimitsConfig>(&zero).is_err(), "{zero}");
        }
    }
}
