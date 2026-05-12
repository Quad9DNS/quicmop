use std::path::PathBuf;

use clap::{ArgAction, Parser, crate_version};

#[derive(Parser)]
#[command(version = quicmop_collector_version(), about, long_about = None, rename_all = "kebab-case")]
pub struct CliArgs {
    /// Increase log verbosity. May be repeated for further increase.
    #[clap(short, long, action = ArgAction::Count)]
    pub verbose: u8,

    /// Decrease log verbosity. May be repeated for further decrease.
    #[clap(short, long, action = ArgAction::Count)]
    pub quiet: u8,

    /// Path to configuration file.
    #[clap(short, long, default_value = "/etc/quicmop/quicmop-collector.yaml")]
    pub config: PathBuf,

    /// Port for the collector gRPC server.
    #[clap(short = 'p', long = "port")]
    pub grpc_server_port: Option<u16>,

    /// Port for the collector gRPC server.
    #[clap(short = 'a', long = "address")]
    pub grpc_server_addr: Option<String>,

    /// Port for the netobserv collector gRPC server, making this collector act as a collector for
    /// netobserv eBPF agent.
    #[clap(long = "netobserv_collector_port")]
    pub netobserv_grpc_server_port: Option<u16>,

    /// Port for the netobserv collector gRPC server, making this collector act a a collector for
    /// netobserv eBPF agent.
    #[clap(long = "netobserv_collector_address")]
    pub netobserv_grpc_server_addr: Option<String>,

    /// Prefix to add to all metrics names.
    #[clap(long, default_value = "quicmop")]
    pub metrics_name_prefix: String,

    /// Netmask to use for source IPv4 addresses
    #[clap(long)]
    pub v4_src_netmask: Option<u8>,

    /// Netmask to use for source IPv6 addresses
    #[clap(long)]
    pub v6_src_netmask: Option<u8>,

    /// Netmask to use for destination IPv4 addresses
    #[clap(long)]
    pub v4_dst_netmask: Option<u8>,

    /// Netmask to use for destination IPv6 addresses
    #[clap(long)]
    pub v6_dst_netmask: Option<u8>,
}

fn quicmop_collector_version() -> String {
    let features: &[&str] = &[];
    format!(
        "{}\nCompiled with: {}",
        crate_version!(),
        features.join(", ")
    )
}
