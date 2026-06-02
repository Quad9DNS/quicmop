use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use crate::proto::{Ip, ip::IpFamily};

pub mod proto {
    tonic::include_proto!("pbflow");
}

impl From<IpFamily> for IpAddr {
    fn from(value: IpFamily) -> Self {
        match value {
            IpFamily::Ipv4(ip) => IpAddr::V4(Ipv4Addr::from_bits(ip)),
            IpFamily::Ipv6(ip) => IpAddr::V6(Ipv6Addr::from_octets(ip[0..16].try_into().unwrap())),
        }
    }
}

impl From<IpAddr> for Ip {
    fn from(value: IpAddr) -> Self {
        match value {
            IpAddr::V4(ipv4_addr) => Self {
                ip_family: Some(IpFamily::Ipv4(ipv4_addr.to_bits())),
            },
            IpAddr::V6(ipv6_addr) => Self {
                ip_family: Some(IpFamily::Ipv6(ipv6_addr.octets().to_vec())),
            },
        }
    }
}

impl TryFrom<Ip> for IpAddr {
    type Error = ();

    fn try_from(value: Ip) -> Result<Self, Self::Error> {
        match value.ip_family {
            Some(IpFamily::Ipv4(ip)) => Ok(IpAddr::V4(Ipv4Addr::from_bits(ip))),
            Some(IpFamily::Ipv6(ip)) => Ok(IpAddr::V6(Ipv6Addr::from_octets(
                ip[0..16].try_into().map_err(|_| ())?,
            ))),
            None => Err(()),
        }
    }
}
