//! The query parameters of a request a broker forwarded to a context source, as a stub journal lists them: each
//! parameter's values, percent-decoded, in the order the request carries them.

use wiremock::Request;

/// The values of the query parameter `name` that `request` carries, decoded, in order; empty when it carries none.
#[must_use]
pub fn query_values(request: &Request, name: &str) -> Vec<String> {
    request
        .url
        .query_pairs()
        .filter(|(key, _)| key == name)
        .map(|(_, value)| value.into_owned())
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::forwarded_query::query_values;
    use googletest::prelude::*;
    use http::{HeaderMap, Method};
    use url::Url;
    use wiremock::Request;

    #[gtest]
    fn the_values_of_one_parameter_are_read_decoded_in_order() -> Result<()> {
        let forwarded = Request {
            url: Url::parse("http://source.test/ngsi-ld/v1/entities?type=Vehicle_a&q=speed%3E1&type=Car")?,
            method: Method::GET,
            headers: HeaderMap::new(),
            body: Vec::new(),
        };
        expect_that!(query_values(&forwarded, "type"), elements_are![eq("Vehicle_a"), eq("Car")]);
        expect_that!(query_values(&forwarded, "q"), elements_are![eq("speed>1")]);
        verify_that!(query_values(&forwarded, "attrs"), is_empty())
    }
}
