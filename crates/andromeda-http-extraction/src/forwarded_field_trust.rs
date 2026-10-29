//! Whether the forwarded fields of one request may be believed.

/// Whether the forwarded fields of a request may decide the scheme and host of
/// the URLs the broker mints from it, carried as a request extension.
///
/// The HTTP stack installs it on every request, from the operator's
/// configuration and the peer the request arrived from. A request carrying
/// none, as one routed by a router assembled without the stack, is treated as
/// [`ForwardedFieldTrust::Untrusted`], so a client-supplied field can never
/// decide a minted URL by default.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ForwardedFieldTrust {
    /// The request arrived from a peer whose forwarded fields are believed.
    Trusted,
    /// The request's forwarded fields are ignored.
    Untrusted,
}
