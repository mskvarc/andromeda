//! The outbound HTTP client every broker-initiated request goes out on.
//!
//! A subscription endpoint and a registration endpoint are both supplied by a
//! client, so an unguarded broker is an SSRF proxy for anything its network
//! reaches. This crate owns the whole defence: which addresses count as private
//! or reserved ([`address_class`]), which addresses an operator allows or denies
//! on top of that ([`address_policy`]), the connect-time resolver that vets every
//! address `reqwest` connects to ([`resolver`]), the pre-send guard that also
//! covers IP-literal URLs the resolver never sees ([`endpoint_guard`]), and the
//! client policy that disables redirects, since a redirect would walk a request
//! past a guard that only vets the original URL ([`client`]).

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

pub mod address_class;
pub mod address_policy;
pub mod client;
pub mod endpoint_guard;
pub mod endpoint_refusal;
pub mod resolver;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: ["andromeda-configuration"],
    allow_workspace_dev_dependencies: [],
    allow_production_dependencies: ["reqwest", "tokio"],
}

#[cfg(test)]
andromeda_architecture_checks::forbid_sql!();
