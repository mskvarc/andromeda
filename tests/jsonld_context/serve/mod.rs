//! Serving a stored `@context` to anyone who dereferences its URL.
//!
//! A remote broker that fetches a Hosted or `ImplicitlyCreated` `@context`
//! from this one holds it as a Cached entry of its own, and clause 6.3.16 holds
//! it to the freshness the Serve response states. The response therefore
//! carries `Cache-Control: max-age=<revalidation span>` and a strong `ETag`
//! over the bytes it sends, answers conditional requests as RFC 7232 requires,
//! and varies by `NGSILD-Tenant`, since one path serves each tenant's own
//! document.

mod conditional;
mod lifetime_configuration;
mod revalidation;
mod tenant_parameter;
mod validators;
