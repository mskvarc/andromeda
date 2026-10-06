//! The query string an entity write is forwarded to a context source with.
//!
//! Clause 4.3.6.1 lets the Entity type selector "be added as a filter on
//! forwarded requests even where its presence initially seems redundant", and
//! Tables 6.5.3.2-1 to 6.7.3.3-1 make every NGSI-LD system support the
//! parameter, so a write forwards the selector the client sent unchanged: its
//! text as written, which the typed parameter has already validated.

use url::form_urlencoded::{Serializer, parse};

/// The `type` parameter of a forwarded write, as the client wrote it in
/// `request_query`, when it sent one.
pub(crate) fn forwarded_type_parameter(request_query: Option<&str>) -> Option<(&'static str, String)> {
    parse(request_query?.as_bytes())
        .find(|(name, _)| name == "type")
        .map(|(_, selector)| ("type", selector.into_owned()))
}

/// The form-encoded query string of the `parameters` a write is forwarded
/// with, or `None` when it is forwarded with none.
pub(crate) fn forwarded_write_query<const N: usize>(parameters: [Option<(&'static str, String)>; N]) -> Option<String> {
    let mut serializer = Serializer::new(String::new());
    serializer.extend_pairs(parameters.into_iter().flatten());
    let query = serializer.finish();
    (!query.is_empty()).then_some(query)
}

#[cfg(test)]
mod tests {
    use crate::forwarded_write_query::{forwarded_type_parameter, forwarded_write_query};

    /// A write forwarded with no parameter carries no query string.
    #[test]
    fn a_write_without_parameters_is_forwarded_without_a_query_string() {
        pretty_assertions::assert_eq!(forwarded_write_query([forwarded_type_parameter(None)]), None);
        pretty_assertions::assert_eq!(forwarded_write_query([forwarded_type_parameter(Some("local=true"))]), None);
    }

    /// Clause 4.3.6.1: the client's selector is forwarded as it was written,
    /// pipe included, beside the parameters the operation forwards itself.
    #[test]
    fn the_selector_is_forwarded_beside_the_operations_own_parameters() {
        pretty_assertions::assert_eq!(
            forwarded_write_query([
                Some(("options", "noOverwrite".to_owned())),
                forwarded_type_parameter(Some("options=noOverwrite&type=Vehicle%7CBuilding")),
            ]),
            Some("options=noOverwrite&type=Vehicle%7CBuilding".to_owned())
        );
    }
}
