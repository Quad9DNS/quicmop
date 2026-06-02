use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use crate::proto::{Ip, SocketMetricsGroup, ip::IpFamily};

pub mod proto {
    tonic::include_proto!("quicmop");
}

impl From<IpFamily> for IpAddr {
    fn from(value: IpFamily) -> Self {
        match value {
            IpFamily::V4(ip) => IpAddr::V4(Ipv4Addr::from_bits(ip)),
            IpFamily::V6(ip) => IpAddr::V6(Ipv6Addr::from_octets(ip[0..16].try_into().unwrap())),
        }
    }
}

impl From<IpAddr> for Ip {
    fn from(value: IpAddr) -> Self {
        match value {
            IpAddr::V4(ipv4_addr) => Self {
                ip_family: Some(IpFamily::V4(ipv4_addr.to_bits())),
            },
            IpAddr::V6(ipv6_addr) => Self {
                ip_family: Some(IpFamily::V6(ipv6_addr.octets().to_vec())),
            },
        }
    }
}

impl TryFrom<Ip> for IpAddr {
    type Error = ();

    fn try_from(value: Ip) -> Result<Self, Self::Error> {
        match value.ip_family {
            Some(IpFamily::V4(ip)) => Ok(IpAddr::V4(Ipv4Addr::from_bits(ip))),
            Some(IpFamily::V6(ip)) => Ok(IpAddr::V6(Ipv6Addr::from_octets(
                ip[0..16].try_into().map_err(|_| ())?,
            ))),
            None => Err(()),
        }
    }
}

pub struct SocketMetrics {
    pub src: IpAddr,
    pub dst: IpAddr,
    pub latency_type: String,
    pub host: String,
    pub metrics: Metrics,
}

pub struct Metrics {
    pub min_rtt_us: u64,
}

impl From<Metrics> for crate::proto::Metrics {
    fn from(value: Metrics) -> Self {
        Self {
            min_rtt_us: value.min_rtt_us,
        }
    }
}

impl From<SocketMetrics> for SocketMetricsGroup {
    fn from(value: SocketMetrics) -> Self {
        Self {
            src: Some(value.src.into()),
            dst: Some(value.dst.into()),
            host: value.host,
            latency_type: value.latency_type,
            metrics: Some(value.metrics.into()),
        }
    }
}
