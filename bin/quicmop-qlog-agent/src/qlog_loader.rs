use std::{
    fs::File,
    io::BufReader,
    net::{IpAddr, SocketAddr},
    path::PathBuf,
    pin::Pin,
    time::Duration,
};

use futures::Stream;
use notify_debouncer_mini::{DebounceEventResult, Debouncer, notify::INotifyWatcher};
use qlog::{
    events::{EventData, http3::Http3Frame},
    reader::Event,
};
use quicmop_proto::proto::{AgentMetricsRequest, Metrics, SocketMetricsGroup};
use tokio::sync::mpsc::{self, Receiver};
use tokio_stream::wrappers::ReceiverStream;

pub struct QlogLoader {
    qlog_dir: PathBuf,
    watcher: Debouncer<INotifyWatcher>,
    metrics_rx: Receiver<AgentMetricsRequest>,
}

type MetricsStream = Pin<Box<dyn Stream<Item = AgentMetricsRequest> + Send>>;

impl QlogLoader {
    pub fn new(hostname: String, qlog_dir: PathBuf) -> Result<Self, Box<dyn std::error::Error>> {
        let http3_schema = qlog::events::HTTP3_URI.to_string();
        let (metrics_tx, metrics_rx) = mpsc::channel(4096);
        Ok(Self {
            qlog_dir,
            watcher: notify_debouncer_mini::new_debouncer(
                Duration::from_secs(5),
                move |res: DebounceEventResult| {
                    if let Ok(events) = res {
                        let mut request = AgentMetricsRequest {
                            metrics: Default::default(),
                        };
                        for event in events {
                            if event.path.is_file()
                                && let Ok(path) = File::open(event.path)
                            {
                                let reader = BufReader::new(path);
                                if let Ok(reader) =
                                    qlog::reader::QlogSeqReader::new(Box::new(reader))
                                {
                                    let latency_type = if reader
                                        .qlog
                                        .trace
                                        .event_schemas
                                        .contains(&http3_schema)
                                    {
                                        "HTTP3"
                                    } else {
                                        "QUIC"
                                    };
                                    // TODO: title is hardcoded to source in our dnsdist impl
                                    let src: Option<IpAddr> =
                                        reader.qlog.title.as_ref().and_then(|f| f.parse().ok());
                                    let mut dst: Option<IpAddr> = None;
                                    for qlog_event in reader {
                                        if let Event::Qlog(qlog) = qlog_event {
                                            if let EventData::Http3FrameParsed(frame_parsed) =
                                                &qlog.data
                                                && let Http3Frame::Headers { headers, raw: _ } =
                                                    &frame_parsed.frame
                                            {
                                                for h in headers {
                                                    if h.name == Some(":authority".to_string()) {
                                                        let socket_addr: Option<SocketAddr> = h
                                                            .value
                                                            .as_ref()
                                                            .and_then(|v| v.parse().ok());
                                                        dst = socket_addr.map(|s| s.ip());
                                                    }
                                                }
                                            }
                                            if let EventData::QuicMetricsUpdated(metrics) =
                                                qlog.data
                                                && let Some(min_rtt) = metrics.min_rtt
                                                &&
                                                let Some(src) = src
                                                    // TODO: we need to figure out DST in qlog
                                                    // too
                                                    && let Some(dst) =
                                                        dst.or_else(|| {
                                                            hostname
                                                                .parse::<IpAddr>()
                                                                .map(Into::into)
                                                                .ok()
                                                                .flatten()
                                                        })
                                            {
                                                request.metrics.push(SocketMetricsGroup {
                                                    src: Some(src.into()),
                                                    dst: Some(dst.into()),
                                                    host: hostname.clone(),
                                                    latency_type: latency_type.to_string(),
                                                    metrics: Some(Metrics {
                                                        min_rtt_us: (min_rtt * 1000.0) as u64,
                                                    }),
                                                });
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        let _ = metrics_tx.blocking_send(request);
                    }
                },
            )?,
            metrics_rx,
        })
    }

    pub fn start_loading(mut self) -> Result<MetricsStream, Box<dyn std::error::Error>> {
        self.watcher.watcher().watch(
            &self.qlog_dir,
            notify_debouncer_mini::notify::RecursiveMode::Recursive,
        )?;
        Box::leak(Box::new(self.watcher));
        Ok(Box::pin(ReceiverStream::new(self.metrics_rx)))
    }
}
