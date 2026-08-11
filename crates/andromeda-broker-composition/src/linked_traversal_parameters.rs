//! The clause 4.5.23 traversal parameters a forwarded linked retrieval carries.
//!
//! Both the entity and the temporal Linked Entity sources ask a remote broker
//! to continue a traversal this broker started, and both express that in the
//! same three parameters: how much further to walk (`join`, `joinLevel`) and
//! which entities the requester already holds (`containedBy`). The window a
//! temporal retrieval adds is its own concern; this is the part that is the
//! traversal itself.

use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_representation::join_mode::JoinMode;
use std::num::NonZeroU32;
use url::form_urlencoded;

/// Append the traversal continuation to a forwarded retrieval's query string.
///
/// `depth` counts the traversal levels still to resolve, the fetched entity's
/// own level included, so the remote broker is asked to join one level less
/// than that. When nothing is left beyond the entity itself the retrieval
/// carries no `join` at all — clause 4.5.23 admits only a positive `joinLevel`,
/// and a zero-hop join is the absence of one.
///
/// `visited` becomes `containedBy`: clause 4.5.23 names it as the entities the
/// requester already holds, which the remote broker must not send back. An
/// empty list is omitted rather than sent empty.
pub(crate) fn append_traversal(serializer: &mut form_urlencoded::Serializer<'_, String>, depth: NonZeroU32, visited: &[Uri]) {
    if let Some(join_level) = NonZeroU32::new(depth.get() - 1) {
        serializer.append_pair("join", &JoinMode::Flat.to_string());
        serializer.append_pair("joinLevel", &join_level.to_string());
    }
    if !visited.is_empty() {
        let held = visited.iter().map(Uri::as_str).collect::<Vec<_>>().join(",");
        serializer.append_pair("containedBy", &held);
    }
}
