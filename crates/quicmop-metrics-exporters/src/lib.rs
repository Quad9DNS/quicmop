use std::{io, sync::Arc};

use file::FileMetricsExporter;
use metrics_exporter_prometheus::PrometheusHandle;
use scrape::ScrapeMetricsExporter;
use stdout::StdoutMetricsExporter;

mod bufwriter;
mod file;
mod scrape;
mod stdout;

pub use file::FileExporterConfig;
pub use scrape::ScrapeExporterConfig;
pub use stdout::StdoutExporterConfig;

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub enum MetricsExporter {
    Stdout(StdoutExporterConfig),
    File(FileExporterConfig),
    Scrape(ScrapeExporterConfig),
}

impl<T: MetricsExtraProvider + 'static> MetricsExporterTaskBuilder<T> for MetricsExporter {
    async fn start_exporting(
        self,
        handle: PrometheusHandle,
        extra_provider: Arc<T>,
    ) -> crate::Result<()> {
        match self {
            MetricsExporter::Stdout(config) => {
                StdoutMetricsExporter::new(config)
                    .start_exporting(handle, extra_provider)
                    .await
            }
            MetricsExporter::File(config) => {
                FileMetricsExporter::new(config)
                    .start_exporting(handle, extra_provider)
                    .await
            }
            MetricsExporter::Scrape(config) => {
                ScrapeMetricsExporter::new(config)
                    .start_exporting(handle, extra_provider)
                    .await
            }
        }
    }
}

impl<T: MetricsExtraProvider + 'static> MetricsExporterBuilder<T> for MetricsExporter {
    fn name(&self) -> String {
        match self {
            MetricsExporter::Stdout(_config) => "stdout".to_string(),
            MetricsExporter::File(config) => {
                let file_name = config.file_path.to_string_lossy();
                format!("file{file_name}")
            }
            MetricsExporter::Scrape(config) => {
                let addr = config.addr;
                format!("http{addr}")
            }
        }
    }
}

#[allow(async_fn_in_trait)]
pub trait MetricsExporterTaskBuilder<T: MetricsExtraProvider> {
    async fn start_exporting(
        self,
        handle: PrometheusHandle,
        extra_provider: Arc<T>,
    ) -> crate::Result<()>;
}

pub trait MetricsExporterBuilder<T: MetricsExtraProvider>: MetricsExporterTaskBuilder<T> {
    #[allow(unused)]
    fn name(&self) -> String;
}

pub trait MetricsExtraProvider: Send + Sync {
    fn render_to_write(&self, output: &mut impl io::Write);
}

pub struct NoopMetricsExtraProvider;

impl MetricsExtraProvider for NoopMetricsExtraProvider {
    fn render_to_write(&self, _: &mut impl io::Write) {}
}
