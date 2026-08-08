//! Entity-query-specific boundary deserializers.

use crate::error::ParameterError;
use andromeda_ngsi_ld_query::ordering::{
    collation::IcuCollation,
    term::{OrderDirection, OrderTerm},
};
use serde::{Deserialize, Deserializer};

/// Deserialize the `collation` query parameter as a validated ICU identifier.
pub(super) fn deserialize_option_collation<'de, D>(deserializer: D) -> Result<Option<IcuCollation>, D::Error>
where
    D: Deserializer<'de>,
{
    let raw: Option<String> = Option::deserialize(deserializer)?;
    raw.map(|value| IcuCollation::parse(&value).map_err(serde::de::Error::custom)).transpose()
}

/// Deserialize `orderBy` into validated query ordering terms.
pub(crate) fn deserialize_optional_order_by<'de, D>(deserializer: D) -> Result<Option<Vec<OrderTerm>>, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = Option::<String>::deserialize(deserializer)?;
    raw.map(|value| parse_order_by(&value).map_err(serde::de::Error::custom)).transpose()
}

/// Parse the comma-separated `orderBy` grammar.
fn parse_order_by(raw: &str) -> Result<Vec<OrderTerm>, ParameterError> {
    raw.split(',')
        .filter_map(|term| {
            let term = term.trim();
            if term.is_empty() {
                return None;
            }
            if let Some((attr, direction)) = term.split_once(';') {
                let direction = match direction.trim() {
                    "asc" => OrderDirection::Asc,
                    "desc" => OrderDirection::Desc,
                    "dist-asc" => OrderDirection::DistAsc,
                    "dist-desc" => OrderDirection::DistDesc,
                    other => return Some(Err(ParameterError::OrderByDirectionInvalid(other.to_owned()))),
                };
                return Some(Ok(OrderTerm {
                    attr: attr.trim().to_owned(),
                    direction,
                }));
            }
            if let Some(attr) = term.strip_prefix('!') {
                return Some(Ok(OrderTerm {
                    attr: attr.to_owned(),
                    direction: OrderDirection::Desc,
                }));
            }
            Some(Ok(OrderTerm {
                attr: term.to_owned(),
                direction: OrderDirection::Asc,
            }))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::deserialize_optional_order_by;
    use andromeda_ngsi_ld_query::ordering::term::{OrderDirection, OrderTerm};
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct Parameters {
        #[serde(rename = "orderBy", deserialize_with = "deserialize_optional_order_by")]
        order_by: Option<Vec<OrderTerm>>,
    }

    #[test]
    fn deserializes_specified_directions() {
        let parsed: Parameters = serde_urlencoded::from_str("orderBy=name,location;dist-desc").expect("valid orderBy");
        let terms = parsed.order_by.expect("order terms");
        pretty_assertions::assert_eq!(terms[0].direction, OrderDirection::Asc);
        pretty_assertions::assert_eq!(terms[1].direction, OrderDirection::DistDesc);
    }

    #[test]
    fn rejects_unknown_direction_at_deserialization_boundary() {
        assert!(serde_urlencoded::from_str::<Parameters>("orderBy=name;sideways").is_err());
    }
}
