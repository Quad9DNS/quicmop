use std::sync::Arc;

use metrics_exporter_prometheus::PrometheusHandle;
use serde::{Deserialize, Serialize};
use tokio::io::{self, BufWriter};

use crate::MetricsExtraProvider;

use super::{MetricsExporterTaskBuilder, bufwriter::BufWriterMetricsExporter};

#[derive(Debug, Clone, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct StdoutExporterConfig {
    #[serde(default = "default_stdout_export_interval")]
    pub export_interval_secs: u64,
}

const fn default_stdout_export_interval() -> u64 {
    15
}

pub struct StdoutMetricsExporter {
    config: StdoutExporterConfig,
}

impl StdoutMetricsExporter {
    pub fn new(config: StdoutExporterConfig) -> Self {
        Self { config }
    }
}

impl<T: MetricsExtraProvider> MetricsExporterTaskBuilder<T> for StdoutMetricsExporter {
    async fn start_exporting(
        self,
        handle: PrometheusHandle,
        extra_provider: Arc<T>,
    ) -> crate::Result<()> {
        BufWriterMetricsExporter::new_with_interval(
            BufWriter::new(io::stdout()),
            self.config.export_interval_secs,
        )
        .start_exporting(handle, extra_provider)
        .await
    }
}
