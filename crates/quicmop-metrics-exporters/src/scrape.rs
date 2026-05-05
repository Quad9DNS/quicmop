use axum::{Router, routing::get};
use std::{future::ready, net::SocketAddr, sync::Arc};
use tracing::debug;

use metrics_exporter_prometheus::PrometheusHandle;
use serde::{Deserialize, Serialize};

use crate::MetricsExtraProvider;

use super::MetricsExporterTaskBuilder;

#[derive(Debug, Clone, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScrapeExporterConfig {
    pub addr: SocketAddr,
}

pub struct ScrapeMetricsExporter {
    config: ScrapeExporterConfig,
}

impl ScrapeMetricsExporter {
    pub fn new(config: ScrapeExporterConfig) -> Self {
        Self { config }
    }
}

impl<T: MetricsExtraProvider + 'static> MetricsExporterTaskBuilder<T> for ScrapeMetricsExporter {
    async fn start_exporting(
        self,
        handle: PrometheusHandle,
        extra_provider: Arc<T>,
    ) -> crate::Result<()> {
        let app = Router::new().route(
            "/metrics",
            get(move || {
                let mut buf = Vec::new();
                handle.render_to_write(&mut buf).unwrap();
                extra_provider.render_to_write(&mut buf);
                ready(String::from_utf8(buf).unwrap())
            }),
        );

        let listener = tokio::net::TcpListener::bind(self.config.addr).await?;
        debug!(
            "Prometheus metrics listening on {}",
            listener.local_addr().unwrap()
        );
        axum::serve(listener, app).await?;
        Ok(())
    }
}
