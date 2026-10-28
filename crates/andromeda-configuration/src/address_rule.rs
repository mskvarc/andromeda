//! One entry of an allow or deny list of outbound destinations.

use ipnet::IpNet;
use schemars::JsonSchema;
use serde::{
    Deserialize,
    Deserializer,
    Serialize,
    de::{Error as DeserializeError, Unexpected, Visitor},
};
use std::{fmt, net::IpAddr, str::FromStr};
use thiserror::Error;

/// An entry that is neither an IP address nor a subnet in CIDR notation.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
#[error("neither an IP address nor a subnet in CIDR notation (`10.2.0.0/24`, `fd00::/8`)")]
pub struct AddressRuleError;

/// An IP address or a subnet, IPv4 or IPv6: one entry of an
/// `allowed_addresses` or `denied_addresses` list.
///
/// A bare address names exactly that host. A host name or a port is not an
/// entry: the lists vet the addresses a name resolves to, and ports are the
/// business of the network policy around the broker. Its wire form is the text
/// (`"10.2.0.0/24"`, `"203.0.113.9"`, `"fd00:1234::/48"`).
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, JsonSchema)]
#[serde(into = "String")]
#[schemars(
    with = "String",
    description = "An IP address (`\"203.0.113.9\"`) or a subnet in CIDR notation (`\"10.2.0.0/24\"`, `\"fd00:1234::/48\"`), IPv4 or IPv6. A host name or a port is refused."
)]
pub struct AddressRule(IpNet);

impl AddressRule {
    /// Whether the address lies inside this entry.
    ///
    /// An IPv4 entry never contains an IPv6 address and the reverse: mapping
    /// between the two families is the caller's decision.
    #[must_use]
    pub fn contains(&self, address: &IpAddr) -> bool {
        self.0.contains(address)
    }
}

impl FromStr for AddressRule {
    type Err = AddressRuleError;

    fn from_str(entry: &str) -> Result<AddressRule, AddressRuleError> {
        entry
            .parse::<IpAddr>()
            .map(IpNet::from)
            .or_else(|_not_an_address| entry.parse::<IpNet>())
            .map(AddressRule)
            .map_err(|_not_a_subnet| AddressRuleError)
    }
}

/// Reads the text of an entry, naming a rejected entry in the error.
struct AddressRuleVisitor;

impl Visitor<'_> for AddressRuleVisitor {
    type Value = AddressRule;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("an IP address or a subnet in CIDR notation, such as `10.2.0.0/24`")
    }

    fn visit_str<E: DeserializeError>(self, entry: &str) -> Result<AddressRule, E> {
        entry.parse().map_err(|_not_an_entry| E::invalid_value(Unexpected::Str(entry), &self))
    }
}

// Written by hand: a derived `try_from` can only report the error type's own
// text, and an error type may not hold the rejected string, so only the
// visitor can name the entry that was refused.
impl<'de> Deserialize<'de> for AddressRule {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<AddressRule, D::Error> {
        deserializer.deserialize_str(AddressRuleVisitor)
    }
}

impl From<AddressRule> for String {
    fn from(rule: AddressRule) -> String {
        rule.0.to_string()
    }
}

#[cfg(test)]
mod tests {
    use crate::address_rule::{AddressRule, AddressRuleError};
    use pretty_assertions::assert_eq;
    use serde::{Deserialize, Serialize};
    use std::{error::Error, net::IpAddr};

    #[derive(Debug, Deserialize, Serialize)]
    struct Holder {
        rules: Vec<AddressRule>,
    }

    fn rule(entry: &str) -> Result<AddressRule, Box<dyn Error>> {
        Ok(entry.parse()?)
    }

    fn address(text: &str) -> Result<IpAddr, Box<dyn Error>> {
        Ok(text.parse()?)
    }

    #[test]
    fn an_ipv4_address_names_exactly_that_host() -> Result<(), Box<dyn Error>> {
        let entry = rule("10.3.0.17")?;
        assert!(entry.contains(&address("10.3.0.17")?));
        assert!(!entry.contains(&address("10.3.0.18")?));
        Ok(())
    }

    #[test]
    fn an_ipv4_subnet_contains_its_hosts_and_nothing_else() -> Result<(), Box<dyn Error>> {
        let entry = rule("10.2.0.0/24")?;
        assert!(entry.contains(&address("10.2.0.5")?));
        assert!(entry.contains(&address("10.2.0.255")?));
        assert!(!entry.contains(&address("10.2.1.0")?));
        assert!(!entry.contains(&address("10.3.0.5")?));
        Ok(())
    }

    #[test]
    fn an_ipv6_address_and_an_ipv6_subnet_are_entries() -> Result<(), Box<dyn Error>> {
        assert!(rule("fd00::1")?.contains(&address("fd00::1")?));
        let subnet = rule("fd00:1234::/48")?;
        assert!(subnet.contains(&address("fd00:1234:0:1::9")?));
        assert!(!subnet.contains(&address("fd00:1235::1")?));
        Ok(())
    }

    #[test]
    fn an_entry_of_one_family_never_contains_an_address_of_the_other() -> Result<(), Box<dyn Error>> {
        assert!(!rule("10.2.0.0/24")?.contains(&address("::ffff:10.2.0.5")?));
        assert!(!rule("::/0")?.contains(&address("10.2.0.5")?));
        Ok(())
    }

    #[test]
    fn a_malformed_entry_is_refused() {
        for entry in [
            "",
            "ten",
            "10.2.0.0/33",
            "fd00::/129",
            "10.2.0.0/",
            "10.2.0/24",
            "example.org",
            "10.0.0.1:8080",
            "[::1]:80",
            "http://10.0.0.1",
        ] {
            assert_eq!(entry.parse::<AddressRule>(), Err(AddressRuleError), "{entry:?}");
        }
    }

    #[test]
    fn a_refused_entry_is_named_in_the_error_that_reads_it() {
        for entry in ["example.org", "10.0.0.1:8080", "10.2.0.0/33"] {
            let outcome = toml::from_str::<Holder>(&format!("rules = [\"10.2.0.0/24\", \"{entry}\"]"));
            assert!(outcome.err().is_some_and(|error| error.to_string().contains(entry)), "{entry}");
        }
    }

    #[test]
    fn a_mixed_list_reads_and_renders_its_entries() -> Result<(), Box<dyn Error>> {
        let holder: Holder = toml::from_str("rules = [\"10.2.0.0/24\", \"10.3.0.17\", \"fd00:1234::/48\", \"::1\"]")?;
        assert_eq!(holder.rules.len(), 4);
        let rendered = toml::to_string(&holder)?;
        assert!(
            rendered.contains("\"10.2.0.0/24\"") && rendered.contains("\"10.3.0.17/32\"") && rendered.contains("\"::1/128\""),
            "{rendered}"
        );
        Ok(())
    }

    #[test]
    fn a_list_with_one_bad_entry_fails_to_parse() {
        let outcome = toml::from_str::<Holder>("rules = [\"10.2.0.0/24\", \"10.2.0.0/40\"]");
        assert!(outcome.is_err(), "{outcome:?}");
    }
}
