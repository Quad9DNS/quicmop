use std::{sync::Arc, time::Duration};

use futures::StreamExt;
use metrics_exporter_prometheus::PrometheusHandle;
use tokio::{
    io::{AsyncWrite, AsyncWriteExt, BufWriter},
    time::interval,
};
use tokio_stream::wrappers::IntervalStream;

use crate::collector::Collector;

use super::MetricsExporterTaskBuilder;

pub struct BufWriterMetricsExporter<W> {
    writer: BufWriter<W>,
    export_interval_secs: u64,
}

impl<W: AsyncWrite> BufWriterMetricsExporter<W> {
    pub fn new_with_interval(writer: BufWriter<W>, interval: u64) -> Self {
        Self {
            writer,
            export_interval_secs: interval,
        }
    }

    pub async fn export(
        self,
        handle: &PrometheusHandle,
        collector: Arc<Collector>,
    ) -> crate::Result<()> {
        let mut writer = Box::pin(self.writer);
        writer.write_all(handle.render().as_bytes()).await?;
        let mut collector_data = Vec::default();
        collector.render_to_write(&mut collector_data);
        writer.write_all(&collector_data).await?;
        writer.flush().await?;
        Ok(())
    }
}

impl<W: AsyncWrite> MetricsExporterTaskBuilder for BufWriterMetricsExporter<W> {
    async fn start_exporting(
        self,
        handle: PrometheusHandle,
        collector: Arc<Collector>,
    ) -> crate::Result<()> {
        let mut intervals =
            IntervalStream::new(interval(Duration::from_secs(self.export_interval_secs)));

        let mut writer = Box::pin(self.writer);
        while (intervals.next().await).is_some() {
            writer
                .write_all(handle.render().as_bytes())
                .await
                .expect("Metrics write failed");
            let mut collector_data = Vec::default();
            collector.render_to_write(&mut collector_data);
            writer.write_all(&collector_data).await?;
            writer.flush().await.expect("Flush failed");
        }

        Ok(())
    }
}
