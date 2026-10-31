//! The `Via` header a broker sends a context source with a forwarded request (clause 6.3.18), as the loop detection
//! cases read it from the requests the source received.

use andromeda_test_suite::context_source::ContextSource;
use googletest::prelude::*;
use http::header::VIA;

/// Waits until the source holds `count` requests and answers the `Via` of the last, when it carries one.
pub(crate) async fn forwarded_via_after(source: &ContextSource, count: usize) -> Result<Option<String>> {
    let received = source.wait_for(count).await?;
    Ok(received
        .last()
        .and_then(|request| request.headers.get(VIA))
        .and_then(|via| via.to_str().ok())
        .map(str::to_owned))
}

/// [`forwarded_via_after`] the first forwarded request.
pub(crate) async fn forwarded_via(source: &ContextSource) -> Result<Option<String>> {
    forwarded_via_after(source, 1).await
}
