use std::{
    collections::HashMap,
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
            let default_tuple = reader
                .qlog
                .trace
                .common_fields
                .as_ref()
                .and_then(|c| c.tuple.clone())
                .unwrap_or_default();
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
            let dst: Option<IpAddr> = hostname.parse::<IpAddr>().map(Into::into).ok().flatten();
            let mut tuples: HashMap<String, (Option<IpAddr>, Option<IpAddr>)> = HashMap::default();
            tuples.insert(default_tuple.clone(), (src, dst));
            for qlog_event in reader {
                if let Event::Qlog(qlog) = qlog_event {
                    let tuple = qlog
                        .ex_data
                        .get("tuple")
                        .and_then(|t| t.as_str())
                        .map(ToString::to_string)
                        .unwrap_or(default_tuple.clone());
                    match qlog.data {
                        EventData::Http3FrameParsed(frame_parsed) => {
                            if let Http3Frame::Headers { headers, raw: _ } = &frame_parsed.frame {
                                for h in headers {
                                    if h.name == Some(":authority".to_string()) {
                                        let socket_addr: Option<SocketAddr> =
                                            h.value.as_ref().and_then(|v| v.parse().ok());
                                        tuples.entry(tuple.clone()).or_default().1 =
                                            socket_addr.map(|s| s.ip());
                                    }
                                }
                            }
                        }
                        EventData::QuicMetricsUpdated(metrics) => {
                            let (src, dst) = tuples.get(&tuple).unwrap_or(&(None, None));
                            if let Some(min_rtt) = metrics.min_rtt
                                && let Some(src) = *src
                                && let Some(dst) = *dst
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
                        EventData::QuicTupleAssigned(tuple_assigned) => {
                            tuples.insert(
                                tuple_assigned.tuple_id,
                                // TODO: Should we always use default if tuple data is missing -
                                // this will help us collect more metrics, but it might be incorrect
                                (
                                    tuple_assigned
                                        .tuple_local
                                        .and_then(|t| {
                                            t.ip_v4
                                                .and_then(|ip| ip.parse().ok())
                                                .or(t.ip_v6.and_then(|ip| ip.parse().ok()))
                                        })
                                        .or(src),
                                    tuple_assigned
                                        .tuple_remote
                                        .and_then(|t| {
                                            t.ip_v4
                                                .and_then(|ip| ip.parse().ok())
                                                .or(t.ip_v6.and_then(|ip| ip.parse().ok()))
                                        })
                                        .or(dst),
                                ),
                            );
                        }
                        _ => {}
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
