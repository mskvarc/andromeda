//! The `Via` header a broker sends a context source with a forwarded request (clause 6.3.18), as the loop detection
//! cases read it from the requests the source received.

use crate::distributed_operations::{fork_rule::ForkRule, redirected_request::wait_for_request};
use andromeda_test_suite::context_source::ContextSource;
use googletest::prelude::*;
use http::header::VIA;

/// Waits until the source holds a request `rule` matches and answers the `Via` of the last of them, when it carries
/// one.
pub(crate) async fn forwarded_via(source: &ContextSource, rule: &ForkRule) -> Result<Option<String>> {
    let request = wait_for_request(source, rule).await?;
    Ok(request.headers.get(VIA).and_then(|via| via.to_str().ok()).map(str::to_owned))
}
