use std::path::PathBuf;

use clap::{ArgAction, Parser, crate_version};

#[derive(Parser)]
#[command(version = quicmop_qlog_agent_version(), about, long_about = None, rename_all = "kebab-case")]
pub struct CliArgs {
    /// Increase log verbosity. May be repeated for further increase.
    #[clap(short, long, action = ArgAction::Count)]
    pub verbose: u8,

    /// Decrease log verbosity. May be repeated for further decrease.
    #[clap(short, long, action = ArgAction::Count)]
    pub quiet: u8,

    /// Path to configuration file.
    #[clap(short, long, default_value = "/etc/quicmop/quicmop-qlog-agent.yaml")]
    pub config: PathBuf,

    /// Port for the collector gRPC server.
    #[clap(short = 'p', long = "collector_port")]
    pub collector_port: Option<u16>,

    /// Hostname for the collector gRPC server.
    #[clap(short = 'a', long = "collector_hostname")]
    pub collector_hostname: Option<String>,

    /// Hostname override for messages to collector
    #[clap(long)]
    pub hostname: Option<String>,

    /// qlog dir override for messages to collector
    #[clap(
        long,
        default_value = "/var/lib/quicmop/quicmop-qlog-agent",
        env = "QLOGDIR"
    )]
    pub qlog_dir: PathBuf,

    /// Prefix to add to all metrics names.
    #[clap(long, default_value = "quicmop_qlog_agent")]
    pub metrics_name_prefix: String,
}

fn quicmop_qlog_agent_version() -> String {
    let features: &[&str] = &[];
    format!(
        "{}\nCompiled with: {}",
        crate_version!(),
        features.join(", ")
    )
}
