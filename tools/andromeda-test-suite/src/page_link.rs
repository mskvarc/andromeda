//! The pages a page links (clause 6.3.10), read by the query parameters their URLs carry: a broker paginating by
//! cursor or by offset names its position in a parameter of each link.

use googletest::{
    description::Description,
    matcher::{Matcher, MatcherBase, MatcherResult},
};
use ngsi_ld::http::Page;
use url::Url;

/// The URLs of the next, previous, first and last pages `page` links, in that order.
#[must_use]
pub fn links<T>(page: &Page<T>) -> Vec<&Url> {
    [page.next(), page.prev(), page.first(), page.last()].into_iter().flatten().collect()
}

/// A requirement on the query parameters of a linked URL.
#[derive(Debug, Clone, MatcherBase)]
pub struct ParameterMatcher {
    /// The parameter's name.
    name: String,
    /// The value it must have, or `None` for any.
    value: Option<String>,
}

/// A URL carrying the query parameter `name`, with any value.
#[must_use]
pub fn carries_parameter(name: &str) -> ParameterMatcher {
    ParameterMatcher {
        name: name.to_owned(),
        value: None,
    }
}

/// A URL carrying the query parameter `name` with the value `value`.
#[must_use]
pub fn carries(name: &str, value: &str) -> ParameterMatcher {
    ParameterMatcher {
        name: name.to_owned(),
        value: Some(value.to_owned()),
    }
}

impl Matcher<&Url> for ParameterMatcher {
    fn matches(&self, actual: &Url) -> MatcherResult {
        actual
            .query_pairs()
            .any(|(name, value)| name == self.name.as_str() && self.value.as_ref().is_none_or(|expected| value == expected.as_str()))
            .into()
    }

    fn describe(&self, result: MatcherResult) -> Description {
        let parameter = match &self.value {
            Some(value) => format!("{}={value}", self.name),
            None => self.name.clone(),
        };
        match result {
            MatcherResult::Match => format!("is a URL carrying the query parameter {parameter}").into(),
            MatcherResult::NoMatch => format!("isn't a URL carrying the query parameter {parameter}").into(),
        }
    }
}

// The URLs `links` collects are references, which a container matcher hands on by reference once more.
impl Matcher<&&Url> for ParameterMatcher {
    fn matches(&self, actual: &&Url) -> MatcherResult {
        Matcher::<&Url>::matches(self, *actual)
    }

    fn describe(&self, result: MatcherResult) -> Description {
        Matcher::<&Url>::describe(self, result)
    }
}

/// A URL whose text holds a given text, such as the name of the resource it reads.
#[derive(Debug, Clone, MatcherBase)]
pub struct NamingMatcher {
    /// The text the URL holds.
    text: String,
}

/// A URL whose text holds `text` anywhere.
#[must_use]
pub fn naming(text: &str) -> NamingMatcher {
    NamingMatcher { text: text.to_owned() }
}

impl Matcher<&Url> for NamingMatcher {
    fn matches(&self, actual: &Url) -> MatcherResult {
        actual.as_str().contains(self.text.as_str()).into()
    }

    fn describe(&self, result: MatcherResult) -> Description {
        match result {
            MatcherResult::Match => format!("is a URL holding {:?}", self.text).into(),
            MatcherResult::NoMatch => format!("isn't a URL holding {:?}", self.text).into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::page_link::{carries, carries_parameter, naming};
    use googletest::prelude::*;
    use url::Url;

    #[gtest]
    fn a_parameter_is_matched_by_name_and_by_value() -> Result<()> {
        let url = Url::parse("http://broker.test/ngsi-ld/v1/entities?type=T&offset=20&limit=2")?;
        expect_that!(&url, carries_parameter("offset"));
        expect_that!(&url, carries("offset", "20"));
        expect_that!(&url, not(carries("offset", "2")));
        verify_that!(&url, not(carries_parameter("cursor")))
    }

    #[gtest]
    fn a_url_names_the_text_it_holds() -> Result<()> {
        let url = Url::parse("http://broker.test/ngsi-ld/v1/csourceSubscriptions?offset=2")?;
        expect_that!(&url, naming("csourceSubscriptions"));
        expect_that!(&url, naming("/csourceSubscriptions"));
        verify_that!(&url, not(naming("/subscriptions")))
    }

    #[gtest]
    fn the_links_of_a_page_are_read_one_by_one() -> Result<()> {
        let next = Url::parse("http://broker.test/ngsi-ld/v1/entities?cursor=abc")?;
        let first = Url::parse("http://broker.test/ngsi-ld/v1/entities")?;
        let links = vec![&next, &first];
        expect_that!(links, contains(carries_parameter("cursor")));
        verify_that!(links, each(not(carries_parameter("offset"))))
    }
}
