//! The `[federation.forwarding]` table: how requests are forwarded to Context
//! Sources.

use crate::{address_rule::AddressRule, human_duration::HumanDuration, schema::private_addresses::PrivateAddresses};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{num::NonZeroUsize, time::Duration};

/// How requests are forwarded to Context Sources.
#[derive(Deserialize, Serialize, JsonSchema, Debug, Clone, PartialEq, Eq)]
#[schemars(deny_unknown_fields)]
#[schemars(description = "How requests reach context sources: fan-out, timeouts and the addresses a source may resolve into.")]
pub struct ForwardingConfig {
    /// The most forwarded requests in flight at once.
    #[schemars(description = "The most forwarded requests one operation has in flight at once.")]
    pub max_concurrent: NonZeroUsize,
    /// How long connecting to a Context Source may take, in human-readable
    /// form (`"3s"`).
    #[schemars(description = "How long connecting to a context source may take.")]
    pub connect_timeout: HumanDuration,
    /// How long a forwarded request may take before it is taken to have
    /// failed, unless the registration's `management.timeout` says otherwise
    /// (clause 5.2.34), in human-readable form (`"5s"`).
    #[schemars(
        description = "How long a forwarded request may take before it counts as failed, connecting and reading the answer included. A registration's own `management.timeout` takes precedence."
    )]
    pub request_timeout: HumanDuration,
    /// Whether a Context Source whose endpoint resolves into private or
    /// reserved address space is `allowed` or `refused`. Checked against the
    /// address actually resolved. Redirects are never followed.
    #[schemars(
        description = "Whether a context source whose endpoint resolves into private or reserved address space is contacted. Checked against the address actually resolved, not the host name. `federation.forwarding.allowed_addresses` exempts addresses from `\"refused\"`, and `federation.forwarding.denied_addresses` refuses addresses whatever this is set to. A redirect from a context source is never followed, whatever this is set to. Allowed by default because context sources commonly run beside the broker on a private network; set `\"refused\"` when clients you do not trust create registrations."
    )]
    pub private_addresses: PrivateAddresses,
    /// Addresses and subnets a Context Source may resolve into even when
    /// `private_addresses` is `refused`.
    #[schemars(
        description = "IP addresses and subnets (`\"10.8.0.0/16\"`, `\"10.3.0.17\"`, `\"fd00:1234::/48\"`) that a context source may resolve into even though `federation.forwarding.private_addresses` is `\"refused\"`. Empty by default. An entry in `federation.forwarding.denied_addresses` takes precedence."
    )]
    pub allowed_addresses: Vec<AddressRule>,
    /// Addresses and subnets a Context Source is never contacted at.
    #[schemars(
        description = "IP addresses and subnets (`\"203.0.113.0/24\"`, `\"198.51.100.9\"`) that a context source must not resolve into, public ones included and whatever `federation.forwarding.private_addresses` is set to. Empty by default. Takes precedence over `federation.forwarding.allowed_addresses`."
    )]
    pub denied_addresses: Vec<AddressRule>,
}

impl Default for ForwardingConfig {
    fn default() -> ForwardingConfig {
        ForwardingConfig {
            max_concurrent: NonZeroUsize::MIN.saturating_add(15),
            connect_timeout: HumanDuration::from_secs(3),
            request_timeout: HumanDuration::from_secs(5),
            private_addresses: PrivateAddresses::Allowed,
            allowed_addresses: Vec::new(),
            denied_addresses: Vec::new(),
        }
    }
}

impl ForwardingConfig {
    /// The connection deadline.
    #[must_use]
    pub const fn connect_timeout(&self) -> Duration {
        self.connect_timeout.duration()
    }

    /// The default per-request deadline.
    #[must_use]
    pub const fn request_timeout(&self) -> Duration {
        self.request_timeout.duration()
    }
}
