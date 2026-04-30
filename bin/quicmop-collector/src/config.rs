use std::{
    collections::HashSet,
    fs::File,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    time::Duration,
};

use serde::{Deserialize, Serialize};
use snafu::ResultExt;
use tracing::Level;

use crate::{
    cli::CliArgs,
    error::{ConfigYamlParsingSnafu, FileReadSnafu},
    metrics_exporters::{
        FileExporterConfig, MetricsExporter, ScrapeExporterConfig, StdoutExporterConfig,
    },
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileBasedConfig {
    #[serde(default)]
    input: InputConfig,
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
            input: self.input.clone(),
            metrics: self.metrics.build()?,
            process: self.process.build()?,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputConfig {
    pub grpc_server_port: u16,
    pub grpc_server_addr: IpAddr,
}

impl Default for InputConfig {
    fn default() -> Self {
        Self {
            grpc_server_port: 8765,
            grpc_server_addr: IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct OutputConfig {}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MetricsConfig {
    #[serde(default)]
    file: Option<FileExporterConfig>,
    #[serde(default)]
    stdout: Option<StdoutExporterConfig>,
    #[serde(default)]
    scrape: Option<ScrapeExporterConfig>,
    #[serde(default = "default_metrics_prefix")]
    name_prefix: String,
    #[serde(default = "default_buckets")]
    buckets: Vec<f64>,
}

impl Default for MetricsConfig {
    fn default() -> Self {
        Self {
            file: None,
            stdout: None,
            scrape: Some(ScrapeExporterConfig {
                addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 9000),
            }),
            name_prefix: default_metrics_prefix(),
            buckets: default_buckets(),
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
            exporters,
            prefix: self.name_prefix.clone(),
            buckets: self.buckets.clone(),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessConfig {
    #[serde(default = "default_thread_count")]
    threads: usize,
    #[serde(default = "default_log_level")]
    log_level: String,
    #[serde(default = "default_shutdown_duration_ms")]
    shutdown_timeout_ms: usize,
}

impl Default for ProcessConfig {
    fn default() -> Self {
        Self {
            threads: default_thread_count(),
            log_level: default_log_level(),
            shutdown_timeout_ms: default_shutdown_duration_ms(),
        }
    }
}

impl ProcessConfig {
    pub fn build(&self) -> crate::Result<ValidatedProcessConfig> {
        Ok(ValidatedProcessConfig {
            threads: self.threads,
            log_level: self.log_level.parse()?,
            shutdown_timeout: Duration::from_millis(self.shutdown_timeout_ms.try_into()?),
        })
    }
}

fn default_log_level() -> String {
    "info".to_string()
}

fn default_thread_count() -> usize {
    std::thread::available_parallelism()
        .map(|r| r.get())
        .unwrap_or(1)
}

fn default_shutdown_duration_ms() -> usize {
    60 * 1000
}

fn default_shutdown_duration() -> Duration {
    Duration::from_secs(60)
}

fn default_metrics_prefix() -> String {
    "quicmop".to_string()
}

fn default_buckets() -> Vec<f64> {
    vec![
        0.5, 1.0, 2.0, 4.0, 8.0, 16.0, 32.0, 64.0, 128.0, 256.0, 512.0, 1024.0, 2048.0, 4096.0,
    ]
}

/// Parsed and validated process configuration for the stringsimile service.
#[derive(Debug, Clone)]
pub struct ValidatedProcessConfig {
    /// Number of threads to use
    pub threads: usize,
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
            threads: if other.threads == default_thread_count() {
                self.threads
            } else {
                other.threads
            },
            log_level: self.log_level.max(other.log_level),
            shutdown_timeout: if other.shutdown_timeout == default_shutdown_duration() {
                self.shutdown_timeout
            } else {
                other.shutdown_timeout
            },
        }
    }
}

/// Configuration for stringsimile metrics.
#[derive(Debug, Clone)]
pub struct ValidatedMetricsConfig {
    /// List of metrics exporters to export metrics with.
    pub exporters: HashSet<MetricsExporter>,
    /// List of buckets to store metrics in.
    pub buckets: Vec<f64>,
    /// Prefix to apply to all metrics names.
    pub prefix: String,
}

impl ValidatedMetricsConfig {
    pub fn merge(self, other: Self) -> Self {
        Self {
            exporters: self.exporters.into_iter().chain(other.exporters).collect(),
            buckets: if other.buckets.is_empty() {
                self.buckets
            } else {
                other.buckets
            },
            prefix: if other.prefix == default_metrics_prefix() {
                self.prefix
            } else {
                other.prefix
            },
        }
    }
}

/// Parsed and validated configuration for the stringsimile service.
#[derive(Debug, Clone)]
pub struct ServiceConfig {
    /// Configuration for input.
    pub input: InputConfig,
    /// Configuration for metrics.
    pub metrics: ValidatedMetricsConfig,
    /// Configuration for the process.
    pub process: ValidatedProcessConfig,
}

impl ServiceConfig {
    pub fn merge(self, other: Self) -> Self {
        Self {
            input: self.input,
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
        let file_config: FileBasedConfig =
            serde_yaml::from_reader(File::open(value.config.clone()).context(FileReadSnafu)?)
                .context(ConfigYamlParsingSnafu)?;
        let base_config = file_config.build()?;

        let log_level_increase = value.verbose - value.quiet;
        let current_log_level = base_config.process.log_level.into_u8();
        let new_log_level = Level::from_u8(current_log_level.saturating_add(log_level_increase));

        let mut input_config = base_config.input.clone();
        if let Some(port) = value.grpc_server_port {
            input_config.grpc_server_port = port;
        }
        if let Some(addr) = value.grpc_server_addr
            && let Ok(addr) = addr.parse()
        {
            input_config.grpc_server_addr = addr;
        }

        let process_config = ValidatedProcessConfig {
            threads: value.threads.unwrap_or(default_thread_count()),
            // Any default for now, will be replaced with the calculated level
            log_level: Level::INFO,
            shutdown_timeout: default_shutdown_duration(),
        };

        let metrics_config = ValidatedMetricsConfig {
            exporters: HashSet::new(),
            prefix: value.metrics_name_prefix,
            buckets: Vec::default(),
        };

        let cli_config = ServiceConfig {
            input: input_config,
            metrics: metrics_config,
            process: process_config,
        };

        let mut config = base_config.merge(cli_config);
        config.process.log_level = new_log_level;

        Ok(config)
    }
}
