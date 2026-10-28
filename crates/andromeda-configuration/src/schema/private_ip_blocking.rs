//! Whether an outbound request may reach private or reserved address space.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use strum::{Display, EnumString};

/// Whether an outbound request may go to an endpoint that resolves into
/// private or reserved address space.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, Display, EnumString, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case", ascii_case_insensitive)]
#[schemars(description = "Whether an outbound request may go to an endpoint that resolves into private or reserved address space.")]
pub enum PrivateIpBlocking {
    /// Any endpoint the broker's network reaches is contacted.
    #[default]
    #[schemars(description = "Any endpoint the broker's network reaches is contacted.")]
    Allowed,
    /// An endpoint whose resolved address is loopback, private or reserved is
    /// refused, so a client-supplied endpoint cannot aim the broker at
    /// internal infrastructure.
    #[schemars(
        description = "An endpoint whose resolved address is loopback, private or reserved is refused, so a client-supplied endpoint cannot aim the broker at internal hosts. The refusal is recorded as a failed request."
    )]
    Refused,
}

#[cfg(test)]
mod tests {
    use crate::schema::private_ip_blocking::PrivateIpBlocking;
    use pretty_assertions::assert_eq;
    use std::str::FromStr;

    #[test]
    fn private_ip_blocking_reads_its_two_values_and_refuses_any_other() {
        assert_eq!(PrivateIpBlocking::from_str("refused"), Ok(PrivateIpBlocking::Refused));
        assert_eq!(PrivateIpBlocking::from_str("allowed"), Ok(PrivateIpBlocking::Allowed));
        assert!(PrivateIpBlocking::from_str("enabled").is_err());
    }
}
