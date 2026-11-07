//! The one decoding of a request's query string into its operation's parameter
//! set (clause 6.3.20).
//!
//! Every parameter name is read into the typed [`QueryParameter`] vocabulary
//! and checked against the operation's [`QueryParameterSet`] before any value
//! is decoded, so a parameter the operation does not define is refused by name.
//! Every value is then held to the text the broker can hold, so a value no
//! store statement could carry is refused by name as well, on every operation.
//!
//! A value that fails is named on the failure path alone. A decoding that
//! tracks the member it is in names it directly. A parameter read inside a
//! flattened group (the page, the geoquery, the ordering) is decoded from a
//! buffer that hides its name, so the parameter whose value alone fails the
//! same way the whole query did is named instead. A failure no single
//! parameter reproduces is a combination of parameters and names none.

use crate::query_refusal::QueryRefusal;
use andromeda_ngsi_ld_number::held_text::admit_held_text;
use andromeda_ngsi_ld_request::query_parameter::{QueryParameter, QueryParameterSet};
use form_urlencoded::Serializer as PairWriter;
use serde::de::DeserializeOwned;
use serde_path_to_error::{Deserializer as PathTracker, Segment, Track};
use serde_urlencoded::{Deserializer as PairReader, de::Error as UrlencodedError};

/// Decode `query` into the parameter set `T`.
///
/// # Errors
///
/// Returns [`QueryRefusal::Undefined`] for a name outside the vocabulary,
/// [`QueryRefusal::NotDefinedHere`] for one `T` does not define,
/// [`QueryRefusal::Unholdable`] for a value the broker cannot hold,
/// [`QueryRefusal::Value`] for a value that does not decode, and
/// [`QueryRefusal::Combination`] for parameters that only fail together.
pub fn decode_query<T>(query: &str) -> Result<T, QueryRefusal>
where
    T: DeserializeOwned + QueryParameterSet,
{
    for (name, value) in form_urlencoded::parse(query.as_bytes()) {
        let parameter: QueryParameter = name.parse().map_err(|_undefined| QueryRefusal::Undefined)?;
        if !T::defines(parameter) {
            return Err(QueryRefusal::NotDefinedHere(parameter));
        }
        admit_held_text(&value).map_err(|source| QueryRefusal::Unholdable { parameter, source })?;
    }
    serde_urlencoded::from_str(query).map_err(
        |source| match tracked_parameter::<T>(query).or_else(|| isolated_parameter::<T>(query, &source)) {
            Some(parameter) => QueryRefusal::Value { parameter, source },
            None => QueryRefusal::Combination(source),
        },
    )
}

/// The parameter a member-tracking decoding of `query` fails in, when the
/// failure is inside a member the decoding can see.
fn tracked_parameter<T: DeserializeOwned>(query: &str) -> Option<QueryParameter> {
    let mut track = Track::new();
    T::deserialize(PathTracker::new(PairReader::new(form_urlencoded::parse(query.as_bytes())), &mut track)).err()?;
    match track.path().iter().next()? {
        Segment::Map { key } => key.parse().ok(),
        Segment::Seq { .. } | Segment::Enum { .. } | Segment::Unknown => None,
    }
}

/// The first parameter of `query` whose pairs alone fail to decode with
/// `failure`.
fn isolated_parameter<T: DeserializeOwned>(query: &str, failure: &UrlencodedError) -> Option<QueryParameter> {
    form_urlencoded::parse(query.as_bytes()).find_map(|(name, _value)| {
        let alone = PairWriter::new(String::new())
            .extend_pairs(form_urlencoded::parse(query.as_bytes()).filter(|(other, _value)| *other == name))
            .finish();
        serde_urlencoded::from_str::<T>(&alone)
            .err()
            .filter(|error| error == failure)
            .and_then(|_alike| name.parse().ok())
    })
}

#[cfg(test)]
mod tests {
    use crate::{query_decoding::decode_query, query_refusal::QueryRefusal};
    use andromeda_ngsi_ld_request::{
        empty::EmptyParams,
        entity::GetEntityParams,
        local::LocalOnlyParams,
        query::entity::parameters::EntityQueryParameters,
        query_parameter::QueryParameter,
    };
    use std::fmt::Debug;

    /// The parameter a refusal names, if any.
    fn named<T: Debug>(result: Result<T, QueryRefusal>) -> Option<QueryParameter> {
        match result.expect_err("the query is refused") {
            QueryRefusal::NotDefinedHere(parameter) | QueryRefusal::Value { parameter, .. } | QueryRefusal::Unholdable { parameter, .. } => Some(parameter),
            QueryRefusal::Undefined | QueryRefusal::Combination(_) => None,
        }
    }

    #[test]
    fn a_defined_query_decodes() {
        let params = decode_query::<LocalOnlyParams>("local=true").expect("decodes");
        pretty_assertions::assert_eq!(params.local, Some(true));
    }

    #[test]
    fn a_name_outside_the_vocabulary_is_undefined() {
        assert!(matches!(decode_query::<EntityQueryParameters>("limitt=3"), Err(QueryRefusal::Undefined)));
    }

    #[test]
    fn a_parameter_another_operation_defines_is_named() {
        pretty_assertions::assert_eq!(named(decode_query::<EmptyParams>("limit=3")), Some(QueryParameter::Limit));
        pretty_assertions::assert_eq!(named(decode_query::<LocalOnlyParams>("local=true&count=true")), Some(QueryParameter::Count));
    }

    #[test]
    fn a_value_outside_a_flattened_group_is_named() {
        pretty_assertions::assert_eq!(named(decode_query::<EntityQueryParameters>("count=yes")), Some(QueryParameter::Count));
        pretty_assertions::assert_eq!(named(decode_query::<EntityQueryParameters>("type=Building&q=(")), Some(QueryParameter::Q));
        pretty_assertions::assert_eq!(named(decode_query::<GetEntityParams>("join=sideways")), Some(QueryParameter::Join));
    }

    #[test]
    fn a_value_inside_a_flattened_group_is_named() {
        pretty_assertions::assert_eq!(
            named(decode_query::<EntityQueryParameters>("type=Building&limit=abc")),
            Some(QueryParameter::Limit)
        );
        pretty_assertions::assert_eq!(
            named(decode_query::<EntityQueryParameters>("type=Building&orderBy=name&orderGeometry=Sphere")),
            Some(QueryParameter::OrderGeometry)
        );
    }

    #[test]
    fn a_georel_without_its_geometry_is_named() {
        pretty_assertions::assert_eq!(
            named(decode_query::<EntityQueryParameters>("type=Building&georel=near%3BmaxDistance%3D%3D10")),
            Some(QueryParameter::Georel)
        );
    }

    #[test]
    fn a_refused_value_is_not_quoted_back() {
        for query in ["count=sideways", "join=sideways", "limit=sideways", "orderBy=name&orderGeometry=sideways"] {
            let detail = decode_query::<EntityQueryParameters>(query).expect_err(query).to_string();
            assert!(!detail.contains("sideways"), "{query}: {detail}");
        }
    }

    /// A value holding U+0000, spelled `%00`, is refused by the parameter that
    /// carries it, whichever parameter and operation it is.
    #[test]
    fn a_value_holding_u0000_is_refused_naming_its_parameter() {
        for (query, parameter) in [
            ("type=Building&q=name%3D%3D%22a%00b%22", QueryParameter::Q),
            ("idPattern=urn%3Ax%00", QueryParameter::IdPattern),
            ("type=Building&lang=%00", QueryParameter::Lang),
        ] {
            let refused = decode_query::<EntityQueryParameters>(query);
            assert!(
                matches!(refused, Err(QueryRefusal::Unholdable { parameter: named, .. }) if named == parameter),
                "{query}: {refused:?}"
            );
        }
        assert!(matches!(decode_query::<LocalOnlyParams>("local=%00"), Err(QueryRefusal::Unholdable { .. })));
    }

    /// The escape spelled out in a value is six characters, not U+0000.
    #[test]
    fn a_value_spelling_the_escape_without_the_character_decodes() {
        assert!(decode_query::<EntityQueryParameters>("type=Building&q=name%3D%3D%22%5Cu0001%22").is_ok());
    }
}
