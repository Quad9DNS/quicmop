use snafu::Snafu;

#[derive(Debug, Snafu)]
#[snafu(visibility(pub))]
pub enum QuicmopCollectorServiceError {
    #[snafu(display("Parsing config YAML file failed: {}", source))]
    ConfigYamlParsing { source: serde_yaml::Error },

    #[snafu(display("Invalid IPv4 netmask. Maximum is 32, but found: {}", netmask))]
    InvalidV4Netmask { netmask: u8 },

    #[snafu(display("Invalid IPv6 netmask. Maximum is 128, but found: {}", netmask))]
    InvalidV6Netmask { netmask: u8 },
}
