//! A listing of stored `@context`s read over the two pages its `next` link leads through (clauses 5.13.3.4 and
//! 6.3.10), with the query written as the test gives it: the `limit` parameter is not one the typed List @contexts
//! writes.

use crate::{broker::BrokerClient, problem::answered, unchecked_page, wire::request_to};
use bytes::Bytes;
use googletest::prelude::*;
use http::{Method, Response, StatusCode};
use ngsi_ld::http::headers::RelationType;
use serde::de::IgnoredAny;

/// The first page of a listing and the page its `next` link leads to, both answered 200.
#[derive(Debug)]
pub struct ListingWalk {
    /// The answer to the request.
    first: Response<Bytes>,
    /// The answer to the `next` link of the first page.
    second: Response<Bytes>,
}

impl ListingWalk {
    /// Lists with `query` (the query string of List @contexts, without the `?`) and follows the `next` link of the
    /// answer.
    ///
    /// # Errors
    ///
    /// Fails when either page is not answered 200, the first page links no next page, or a request cannot be built.
    pub async fn read(client: &BrokerClient, query: &str) -> Result<ListingWalk> {
        let first = client
            .send_unchecked(request_to(client, Method::GET, &format!("jsonldContexts?{query}"))?)
            .await;
        verify_that!(&first, answered(StatusCode::OK))?;
        let first = first.or_fail()?;
        let second = unchecked_page::follow(client, &first, &RelationType::Next).await?.or_fail()?;
        verify_that!(&second, answered(StatusCode::OK))?;
        Ok(ListingWalk {
            first,
            second: second.or_fail()?,
        })
    }

    /// Requires that `text` (an entry's identifier or URL) occurs a different number of times on the two pages: they
    /// differ exactly when one of them lists the entry and the other does not.
    ///
    /// # Errors
    ///
    /// Fails when both pages hold `text` as often.
    pub fn require_on_one_page(&self, text: &str) -> Result<()> {
        verify_that!(occurrences(&self.second, text), not(eq(occurrences(&self.first, text))))
    }

    /// The answer to the request.
    #[must_use]
    pub const fn first(&self) -> &Response<Bytes> {
        &self.first
    }

    /// The answer to the `next` link of the first page.
    #[must_use]
    pub const fn second(&self) -> &Response<Bytes> {
        &self.second
    }
}

/// The number of entries of the array a listing answered with.
///
/// # Errors
///
/// Returns the parse error when the body is not a JSON array.
pub fn entry_count(page: &Response<Bytes>) -> Result<usize, serde_json::Error> {
    serde_json::from_slice::<Vec<IgnoredAny>>(page.body()).map(|entries| entries.len())
}

/// How many times `text` occurs in the body of `page`: an entry's identifier occurs on the page that lists it.
#[must_use]
pub fn occurrences(page: &Response<Bytes>, text: &str) -> usize {
    String::from_utf8_lossy(page.body()).matches(text).count()
}

#[cfg(test)]
mod tests {
    use crate::context_listing_walk::{entry_count, occurrences};
    use bytes::Bytes;
    use googletest::prelude::*;
    use http::Response;

    fn page(body: &str) -> Response<Bytes> {
        Response::new(Bytes::from(body.to_owned()))
    }

    #[gtest]
    fn the_entries_of_an_array_page_are_counted() -> Result<()> {
        expect_that!(entry_count(&page(r#"[{"a":1},"b",[2]]"#))?, eq(3));
        expect_that!(entry_count(&page("[]"))?, eq(0));
        verify_that!(entry_count(&page("{}")).is_err(), eq(true))
    }

    #[gtest]
    fn an_identifier_is_counted_each_time_the_page_holds_it() {
        let listed = page(r#"[{"localId":"x1","URL":"u/x1"},{"localId":"x2"}]"#);
        expect_that!(occurrences(&listed, "x1"), eq(2));
        expect_that!(occurrences(&listed, "x3"), eq(0));
    }
}
