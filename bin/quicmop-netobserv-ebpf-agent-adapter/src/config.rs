use std::{
    collections::HashSet,
    fs::File,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    time::Duration,
};

use quicmop_metrics_exporters::{
    FileExporterConfig, MetricsExporter, ScrapeExporterConfig, StdoutExporterConfig,
};
use serde::{Deserialize, Serialize};
use snafu::ResultExt;
use tracing::Level;

use crate::{cli::CliArgs, error::ConfigYamlParsingSnafu};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FileBasedConfig {
    #[serde(default)]
    agent: AgentConfig,
    #[serde(default)]
    output: OutputConfig,
    #[serde(default)]
    metrics: MetricsConfig,
    #[serde(default)]
    process: ProcessConfig,
}

impl FileBasedConfig {
    pub fn build(&self) -> crate::Result<ServiceConfig> {
        Ok(ServiceConfig {
            agent: self.agent.build()?,
            output: self.output.clone(),
            metrics: self.metrics.build()?,
            process: self.process.build()?,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    #[serde(default)]
    pub hostname: Option<String>,
    #[serde(default = "default_netobserv_grcp_server_port")]
    pub netobserv_grpc_server_port: u16,
    #[serde(default = "default_netobserv_grcp_server_addr")]
    pub netobserv_grpc_server_addr: String,
}

impl AgentConfig {
    pub fn build(&self) -> crate::Result<ValidatedAgentConfig> {
        Ok(ValidatedAgentConfig {
            hostname: self.hostname.clone(),
            netobserv_grpc_server_addr: SocketAddr::new(
                self.netobserv_grpc_server_addr.parse()?,
                self.netobserv_grpc_server_port,
            ),
        })
    }
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            hostname: None,
            netobserv_grpc_server_port: default_netobserv_grcp_server_port(),
            netobserv_grpc_server_addr: default_netobserv_grcp_server_addr(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputConfig {
    pub collector_port: u16,
    pub collector_hostname: String,
}

impl Default for OutputConfig {
    fn default() -> Self {
        Self {
            collector_port: 8765,
            collector_hostname: "localhost".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MetricsConfig {
    #[serde(default = "default_metrics_prefix")]
    name_prefix: String,
    #[serde(default)]
    file: Option<FileExporterConfig>,
    #[serde(default)]
    stdout: Option<StdoutExporterConfig>,
    #[serde(default)]
    scrape: Option<ScrapeExporterConfig>,
}

impl Default for MetricsConfig {
    fn default() -> Self {
        Self {
            name_prefix: default_metrics_prefix(),
            file: None,
            stdout: None,
            scrape: Some(ScrapeExporterConfig {
                addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 9000),
            }),
        }
    }
}

impl MetricsConfig {
    pub fn build(&self) -> crate::Result<ValidatedMetricsConfig> {
        let mut exporters = HashSet::default();
        if let Some(config) = &self.file {
            exporters.insert(MetricsExporter::File(config.clone()));
        }
        if let Some(config) = &self.stdout {
            exporters.insert(MetricsExporter::Stdout(config.clone()));
        }
        if let Some(config) = &self.scrape {
            exporters.insert(MetricsExporter::Scrape(config.clone()));
        }

        Ok(ValidatedMetricsConfig {
            prefix: self.name_prefix.clone(),
            exporters,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessConfig {
    #[serde(default = "default_log_level")]
    log_level: String,
    #[serde(default = "default_shutdown_duration_ms")]
    shutdown_timeout_ms: usize,
}

impl Default for ProcessConfig {
    fn default() -> Self {
        Self {
            log_level: default_log_level(),
            shutdown_timeout_ms: default_shutdown_duration_ms(),
        }
    }
}

impl ProcessConfig {
    pub fn build(&self) -> crate::Result<ValidatedProcessConfig> {
        Ok(ValidatedProcessConfig {
            log_level: self.log_level.parse()?,
            shutdown_timeout: Duration::from_millis(self.shutdown_timeout_ms.try_into()?),
        })
    }
}

fn default_log_level() -> String {
    "info".to_string()
}

fn default_shutdown_duration_ms() -> usize {
    60 * 1000
}

fn default_shutdown_duration() -> Duration {
    Duration::from_secs(60)
}

fn default_metrics_prefix() -> String {
    "quicmop_netobserv_ebpf_agent_adapter".to_string()
}

fn default_netobserv_grcp_server_port() -> u16 {
    2055
}

fn default_netobserv_grcp_server_addr() -> String {
    "0.0.0.0".to_string()
}

/// Parsed and validated process configuration for the agent.
#[derive(Debug, Clone)]
pub struct ValidatedAgentConfig {
    /// Hostname override to use in requests to the collector to identify this host.
    pub hostname: Option<String>,
    /// Address to serve gRPC server on to accept netobserv eBPF agent data.
    pub netobserv_grpc_server_addr: SocketAddr,
}

impl ValidatedAgentConfig {
    pub fn merge(self, other: Self) -> Self {
        Self {
            hostname: if let Some(hostname) = other.hostname {
                Some(hostname)
            } else {
                self.hostname
            },
            netobserv_grpc_server_addr: if other.netobserv_grpc_server_addr.port()
                == default_netobserv_grcp_server_port()
                && other.netobserv_grpc_server_addr.ip().to_string()
                    == default_netobserv_grcp_server_addr()
            {
                self.netobserv_grpc_server_addr
            } else {
                other.netobserv_grpc_server_addr
            },
        }
    }
}

/// Parsed and validated process configuration for the quicmop service.
#[derive(Debug, Clone)]
pub struct ValidatedProcessConfig {
    /// Internal logging level
    pub log_level: Level,
    /// Graceful shutdown timeout. When shutdown is requested (SIGINT), the process will wait for
    /// processing to complete for the given duration and will resort to forceful shutdown
    /// afterwards.
    pub shutdown_timeout: Duration,
}

impl ValidatedProcessConfig {
    pub fn merge(self, other: Self) -> Self {
        Self {
            log_level: self.log_level.max(other.log_level),
            shutdown_timeout: if other.shutdown_timeout == default_shutdown_duration() {
                self.shutdown_timeout
            } else {
                other.shutdown_timeout
            },
        }
    }
}

/// Configuration for quicmop metrics.
#[derive(Debug, Clone)]
pub struct ValidatedMetricsConfig {
    /// Prefix to apply to all metrics names.
    pub prefix: String,
    /// List of metrics exporters to export metrics with.
    pub exporters: HashSet<MetricsExporter>,
}

impl ValidatedMetricsConfig {
    pub fn merge(self, other: Self) -> Self {
        Self {
            prefix: if other.prefix == default_metrics_prefix() {
                self.prefix
            } else {
                other.prefix
            },
            exporters: self.exporters.into_iter().chain(other.exporters).collect(),
        }
    }
}

/// Parsed and validated configuration for the quicmop service.
#[derive(Debug, Clone)]
pub struct ServiceConfig {
    /// Configuration for input.
    pub agent: ValidatedAgentConfig,
    /// Configuration for output.
    pub output: OutputConfig,
    /// Configuration for metrics.
    pub metrics: ValidatedMetricsConfig,
    /// Configuration for the process.
    pub process: ValidatedProcessConfig,
}

impl ServiceConfig {
    pub fn merge(self, other: Self) -> Self {
        Self {
            agent: self.agent.merge(other.agent),
            output: other.output,
            metrics: self.metrics.merge(other.metrics),
            process: self.process.merge(other.process),
        }
    }
}

pub trait LevelInt {
    #[must_use]
    fn into_u8(self) -> u8;
    #[must_use]
    fn from_u8(level: u8) -> Self;
}

impl LevelInt for Level {
    fn into_u8(self) -> u8 {
        match self {
            Level::ERROR => 1,
            Level::WARN => 2,
            Level::INFO => 3,
            Level::DEBUG => 4,
            Level::TRACE => 5,
        }
    }

    fn from_u8(level: u8) -> Self {
        match level {
            0 | 1 => Level::ERROR,
            2 => Level::WARN,
            3 => Level::INFO,
            4 => Level::DEBUG,
            _ => Level::TRACE,
        }
    }
}

impl TryFrom<CliArgs> for ServiceConfig {
    type Error = Box<dyn std::error::Error>;

    fn try_from(value: CliArgs) -> crate::Result<ServiceConfig> {
        let file_config: FileBasedConfig = File::open(value.config.clone())
            .map(serde_yaml::from_reader)
            .unwrap_or(Ok(FileBasedConfig::default()))
            .context(ConfigYamlParsingSnafu)?;
        let base_config = file_config.build()?;

        let log_level_increase = value.verbose - value.quiet;
        let current_log_level = base_config.process.log_level.into_u8();
        let new_log_level = Level::from_u8(current_log_level.saturating_add(log_level_increase));

        let agent_config = ValidatedAgentConfig {
            hostname: value.hostname,
            netobserv_grpc_server_addr: SocketAddr::new(
                value
                    .netobserv_grpc_server_addr
                    .unwrap_or(default_netobserv_grcp_server_addr().parse().unwrap()),
                value
                    .netobserv_grpc_server_port
                    .unwrap_or(default_netobserv_grcp_server_port()),
            ),
        };

        let process_config = ValidatedProcessConfig {
            // Any default for now, will be replaced with the calculated level
            log_level: Level::INFO,
            shutdown_timeout: default_shutdown_duration(),
        };

        let mut output_config = base_config.output.clone();
        if let Some(hostname) = value.collector_hostname {
            output_config.collector_hostname = hostname;
        }
        if let Some(port) = value.collector_port {
            output_config.collector_port = port;
        }

        let metrics_config = ValidatedMetricsConfig {
            prefix: value.metrics_name_prefix,
            exporters: HashSet::new(),
        };

        let cli_config = ServiceConfig {
            agent: agent_config,
            output: output_config,
            metrics: metrics_config,
            process: process_config,
        };

        let mut config = base_config.merge(cli_config);
        config.process.log_level = new_log_level;

        Ok(config)
    }
}
