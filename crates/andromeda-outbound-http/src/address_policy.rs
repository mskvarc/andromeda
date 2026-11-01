//! Which resolved addresses an outbound request may be made to.

use crate::{address_class::is_private_ip, endpoint_refusal::EndpointRefusal};
use andromeda_configuration::{address_rule::AddressRule, schema::private_addresses::PrivateAddresses};
use std::{
    net::{IpAddr, Ipv4Addr},
    sync::Arc,
};

/// The first 96 bits of the NAT64 well-known prefix `64:ff9b::/96`.
const NAT64_PREFIX: [u8; 12] = [0x00, 0x64, 0xff, 0x9b, 0, 0, 0, 0, 0, 0, 0, 0];

/// The rules an outbound request's resolved address is judged by, in order:
/// the deny list refuses an address whatever else is set, the allow list
/// exempts an address from the private-address check, and the private-address
/// check decides the rest.
///
/// Cloning shares the lists, so the pre-send guard and the connect-time
/// resolver of one client hold the same rules at no cost.
#[derive(Clone, Debug, Default)]
pub struct AddressPolicy {
    /// Whether loopback, private and reserved addresses are refused.
    private_addresses: PrivateAddresses,
    /// Addresses exempt from the private-address check.
    allowed: Arc<[AddressRule]>,
    /// Addresses refused whatever the private-address check says.
    denied: Arc<[AddressRule]>,
}

impl AddressPolicy {
    /// A policy over the private-address mode and the two lists.
    #[must_use]
    pub fn new(private_addresses: PrivateAddresses, allowed: Vec<AddressRule>, denied: Vec<AddressRule>) -> AddressPolicy {
        AddressPolicy {
            private_addresses,
            allowed: allowed.into(),
            denied: denied.into(),
        }
    }

    /// Whether the policy refuses nothing, so no address needs resolving to
    /// judge an endpoint. An allow list alone refuses nothing: it only
    /// exempts addresses from a `refused` private-address mode.
    #[must_use]
    pub fn is_open(&self) -> bool {
        match self.private_addresses {
            PrivateAddresses::Allowed => self.denied.is_empty(),
            PrivateAddresses::Refused => false,
        }
    }

    /// Judge a resolved address.
    ///
    /// An IPv4-mapped IPv6 address is judged as the IPv4 it embeds, so
    /// `::ffff:10.2.0.5` and `10.2.0.5` get the same answer. A NAT64
    /// (`64:ff9b::/96`) address is also refused when the IPv4 it translates to
    /// is denied, but an allow entry for that IPv4 does not exempt it: only an
    /// entry covering the IPv6 address itself does.
    ///
    /// # Errors
    ///
    /// Returns [`EndpointRefusal::Blocked`] naming the address when it is
    /// denied, or is private or reserved and not allowed.
    pub fn vet(&self, address: IpAddr) -> Result<(), EndpointRefusal> {
        let judged = unmapped(address);
        let translated = nat64_target(&judged);
        let denied = self
            .denied
            .iter()
            .any(|rule| rule.contains(&judged) || translated.is_some_and(|target| rule.contains(&IpAddr::V4(target))));
        let unexempt_private =
            self.private_addresses == PrivateAddresses::Refused && is_private_ip(&judged) && !self.allowed.iter().any(|rule| rule.contains(&judged));
        if denied || unexempt_private {
            Err(EndpointRefusal::Blocked { ip: address })
        } else {
            Ok(())
        }
    }
}

/// The IPv4 an IPv4-mapped IPv6 address embeds, or the address itself.
fn unmapped(address: IpAddr) -> IpAddr {
    match address {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map_or(address, IpAddr::V4),
        IpAddr::V4(_) => address,
    }
}

/// The IPv4 a well-known NAT64 address (RFC 6052, `64:ff9b::/96`) translates to.
fn nat64_target(address: &IpAddr) -> Option<Ipv4Addr> {
    let IpAddr::V6(v6) = address else {
        return None;
    };
    let octets = v6.octets();
    (octets[..12] == NAT64_PREFIX).then(|| Ipv4Addr::new(octets[12], octets[13], octets[14], octets[15]))
}

#[cfg(test)]
mod tests {
    use crate::address_policy::AddressPolicy;
    use andromeda_configuration::{address_rule::AddressRule, schema::private_addresses::PrivateAddresses};
    use std::{error::Error, net::IpAddr};

    fn rules(entries: &[&str]) -> Result<Vec<AddressRule>, Box<dyn Error>> {
        entries.iter().map(|entry| Ok(entry.parse()?)).collect()
    }

    fn address(text: &str) -> Result<IpAddr, Box<dyn Error>> {
        Ok(text.parse()?)
    }

    fn policy(private_addresses: PrivateAddresses, allowed: &[&str], denied: &[&str]) -> Result<AddressPolicy, Box<dyn Error>> {
        Ok(AddressPolicy::new(private_addresses, rules(allowed)?, rules(denied)?))
    }

    #[test]
    fn empty_lists_judge_an_address_as_the_private_address_mode_alone_does() -> Result<(), Box<dyn Error>> {
        let refused = policy(PrivateAddresses::Refused, &[], &[])?;
        assert!(refused.vet(address("10.2.0.5")?).is_err());
        assert!(refused.vet(address("8.8.8.8")?).is_ok());
        let allowed = policy(PrivateAddresses::Allowed, &[], &[])?;
        assert!(allowed.vet(address("10.2.0.5")?).is_ok());
        assert!(allowed.vet(address("127.0.0.1")?).is_ok());
        Ok(())
    }

    #[test]
    fn an_allow_entry_exempts_private_space_from_a_refused_mode_and_nothing_outside_it() -> Result<(), Box<dyn Error>> {
        let policy = policy(PrivateAddresses::Refused, &["10.2.0.0/24", "fd00:1234::/48"], &[])?;
        assert!(policy.vet(address("10.2.0.5")?).is_ok());
        assert!(policy.vet(address("fd00:1234::7")?).is_ok());
        assert!(policy.vet(address("10.3.0.5")?).is_err());
        assert!(policy.vet(address("127.0.0.1")?).is_err());
        Ok(())
    }

    #[test]
    fn a_deny_entry_inside_an_allow_entry_wins() -> Result<(), Box<dyn Error>> {
        let policy = policy(PrivateAddresses::Refused, &["10.2.0.0/24"], &["10.2.0.9"])?;
        assert!(policy.vet(address("10.2.0.5")?).is_ok());
        assert!(policy.vet(address("10.2.0.9")?).is_err());
        Ok(())
    }

    #[test]
    fn a_deny_entry_refuses_a_public_address_when_private_addresses_are_allowed() -> Result<(), Box<dyn Error>> {
        let policy = policy(PrivateAddresses::Allowed, &[], &["203.0.113.0/24", "8.8.8.8", "2001:4860::/32"])?;
        assert!(policy.vet(address("203.0.113.5")?).is_err());
        assert!(policy.vet(address("8.8.8.8")?).is_err());
        assert!(policy.vet(address("2001:4860:4860::8888")?).is_err());
        assert!(policy.vet(address("8.8.4.4")?).is_ok());
        assert!(policy.vet(address("10.2.0.5")?).is_ok());
        Ok(())
    }

    #[test]
    fn an_ipv4_mapped_address_gets_the_answer_of_the_ipv4_it_embeds() -> Result<(), Box<dyn Error>> {
        let allowing = policy(PrivateAddresses::Refused, &["10.2.0.0/24"], &[])?;
        for text in ["10.2.0.5", "::ffff:10.2.0.5"] {
            assert!(allowing.vet(address(text)?).is_ok(), "{text}");
        }
        let denying = policy(PrivateAddresses::Allowed, &[], &["203.0.113.0/24"])?;
        for text in ["203.0.113.5", "::ffff:203.0.113.5"] {
            assert!(denying.vet(address(text)?).is_err(), "{text}");
        }
        Ok(())
    }

    #[test]
    fn an_allow_entry_for_an_ipv4_does_not_exempt_the_nat64_address_that_translates_to_it() -> Result<(), Box<dyn Error>> {
        let policy = policy(PrivateAddresses::Refused, &["10.2.0.0/24"], &[])?;
        assert!(policy.vet(address("64:ff9b::a02:5")?).is_err());
        Ok(())
    }

    #[test]
    fn a_deny_entry_for_an_ipv4_refuses_the_nat64_address_that_translates_to_it() -> Result<(), Box<dyn Error>> {
        let policy = policy(PrivateAddresses::Allowed, &[], &["203.0.113.0/24"])?;
        assert!(policy.vet(address("64:ff9b::cb00:7105")?).is_err());
        assert!(policy.vet(address("64:ff9b::808:808")?).is_ok());
        Ok(())
    }

    #[test]
    fn only_a_policy_that_can_refuse_something_needs_addresses_resolved() -> Result<(), Box<dyn Error>> {
        assert!(policy(PrivateAddresses::Allowed, &[], &[])?.is_open());
        assert!(policy(PrivateAddresses::Allowed, &["10.2.0.0/24"], &[])?.is_open());
        assert!(!policy(PrivateAddresses::Allowed, &[], &["203.0.113.0/24"])?.is_open());
        assert!(!policy(PrivateAddresses::Refused, &[], &[])?.is_open());
        Ok(())
    }
}
