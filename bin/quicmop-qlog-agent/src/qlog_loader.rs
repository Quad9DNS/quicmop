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
use tokio::sync::mpsc::{self, Receiver, Sender};
use tokio_stream::wrappers::ReceiverStream;
use walkdir::WalkDir;

pub struct QlogLoader {
    qlog_dir: PathBuf,
    hostname: String,
    watcher: Debouncer<INotifyWatcher>,
    metrics_rx: Receiver<AgentMetricsRequest>,
    metrics_tx: Sender<AgentMetricsRequest>,
}

type MetricsStream = Pin<Box<dyn Stream<Item = AgentMetricsRequest> + Send>>;

impl QlogLoader {
    pub fn new(hostname: String, qlog_dir: PathBuf) -> Result<Self, Box<dyn std::error::Error>> {
        let (metrics_tx, metrics_rx) = mpsc::channel(4096);
        let metrics_tx_for_debouncer = metrics_tx.clone();
        Ok(Self {
            qlog_dir,
            hostname: hostname.clone(),
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
                                Self::handle_qlog_file(path, hostname.clone(), &mut request);
                            }
                        }
                        let _ = metrics_tx_for_debouncer.blocking_send(request);
                    }
                },
            )?,
            metrics_rx,
            metrics_tx,
        })
    }

    fn handle_qlog_file(file: File, hostname: String, request: &mut AgentMetricsRequest) {
        let reader = BufReader::new(file);
        if let Ok(reader) = qlog::reader::QlogSeqReader::new(Box::new(reader)) {
            let latency_type = if reader
                .qlog
                .trace
                .event_schemas
                .iter()
                // Contains &str doesn't work, so iter().any() is used
                .any(|v| v == qlog::events::HTTP3_URI)
            {
                "HTTP3"
            } else {
                "QUIC"
            };
            // TODO: peer_ip is included in description in our dnsdist impl
            let src: Option<IpAddr> = reader.qlog.description.as_ref().and_then(|desc| {
                let desc_pairs = desc.split(" ");
                for p in desc_pairs {
                    if let Some((key, value)) = p.split_once("=")
                        && key == "peer_ip"
                    {
                        return value.parse().ok();
                    }
                }
                None
            });
            let mut dst: Option<IpAddr> = None;
            for qlog_event in reader {
                if let Event::Qlog(qlog) = qlog_event {
                    if let EventData::Http3FrameParsed(frame_parsed) = &qlog.data
                        && let Http3Frame::Headers { headers, raw: _ } = &frame_parsed.frame
                    {
                        for h in headers {
                            if h.name == Some(":authority".to_string()) {
                                let socket_addr: Option<SocketAddr> =
                                    h.value.as_ref().and_then(|v| v.parse().ok());
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

    pub async fn start_loading(mut self) -> Result<MetricsStream, Box<dyn std::error::Error>> {
        self.watcher.watcher().watch(
            &self.qlog_dir,
            notify_debouncer_mini::notify::RecursiveMode::Recursive,
        )?;
        Box::leak(Box::new(self.watcher));
        let mut initial_request = AgentMetricsRequest {
            metrics: Default::default(),
        };
        for file in WalkDir::new(&self.qlog_dir)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|v| v.file_type().is_file())
        {
            if let Ok(file) = File::open(file.path()) {
                Self::handle_qlog_file(file, self.hostname.clone(), &mut initial_request);
            }
        }
        let _ = self.metrics_tx.send(initial_request).await;
        Ok(Box::pin(ReceiverStream::new(self.metrics_rx)))
    }
}
