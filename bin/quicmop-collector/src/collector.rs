use std::{
    collections::{HashMap, HashSet},
    io,
    net::IpAddr,
    sync::Arc,
    time::{Duration, Instant},
};

use arc_swap::ArcSwap;
use ipnet::IpNet;
use metrics::{Key, Label};
use metrics_exporter_prometheus::{LabelSet, formatting};
use metrics_util::storage::Histogram;
use moka::future::Cache;
use quicmop_proto::proto::{
    AgentMetricsRequest, CollectorResponse,
    quicmop_socket_metrics_service_server::QuicmopSocketMetricsService,
};

#[derive(Clone)]
struct AddressEntry {
    min_rtt_us: u64,
    last_update: Instant,
}

#[derive(Clone, Hash, Eq, PartialEq)]
struct AddressKey {
    src: IpAddr,
    dst: IpAddr,
    latency_type: String,
    host: String,
}

#[derive(Clone, Hash, Eq, PartialEq)]
struct NetworkKey {
    src: IpNet,
    dst: IpNet,
    latency_type: String,
    host: String,
}

impl AddressKey {
    fn size(&self) -> u32 {
        ((if self.src.is_ipv4() { 4 } else { 16 })
            + (if self.dst.is_ipv4() { 4 } else { 16 })
            + self.latency_type.len()
            + self.host.len())
        .try_into()
        .unwrap_or(u32::MAX)
    }
}

pub struct Collector {
    v4_netmask: u8,
    v6_netmask: u8,
    buckets: Vec<f64>,
    addresses: Cache<AddressKey, AddressEntry>,
    metrics: Arc<ArcSwap<HashMap<NetworkKey, (Histogram, Instant)>>>,
    timeout: Duration,
    bucket_name: String,
    unique_addresses_name: String,
}

impl Collector {
    pub fn new(v4_netmask: u8, v6_netmask: u8, buckets: Vec<f64>, name_prefix: String) -> Self {
        Self {
            v4_netmask,
            v6_netmask,
            buckets,
            addresses: Cache::builder()
                .weigher(|k: &AddressKey, _| -> u32 { k.size() + size_of::<AddressEntry>() as u32 })
                .max_capacity(32 * 1024 * 1024) // 32 MiB
                .time_to_live(Duration::from_secs(60))
                .build(),
            metrics: Arc::new(ArcSwap::new(Arc::new(HashMap::default()))),
            timeout: Duration::from_secs(60),
            bucket_name: format!("{name_prefix}_bucket"),
            unique_addresses_name: format!("{name_prefix}_unique_addresses"),
        }
    }

    pub fn render_to_write(&self, output: &mut impl io::Write) {
        let mut histograms = (**self.metrics.load()).clone();

        let mut unique_addresses: HashMap<NetworkKey, HashSet<IpAddr>> = HashMap::new();

        for (key, entry) in self.addresses.iter() {
            let src_net = IpNet::new(
                key.src,
                if key.src.is_ipv4() {
                    self.v4_netmask
                } else {
                    self.v6_netmask
                },
            )
            .unwrap()
            .trunc();
            let dst_net = IpNet::new(
                key.dst,
                if key.dst.is_ipv4() {
                    self.v4_netmask
                } else {
                    self.v6_netmask
                },
            )
            .unwrap()
            .trunc();
            let net_key = NetworkKey {
                src: src_net,
                dst: dst_net,
                latency_type: key.latency_type.clone(),
                host: key.host.clone(),
            };
            let x = histograms
                .entry(net_key.clone())
                .or_insert((Histogram::new(&self.buckets).unwrap(), Instant::now()));
            x.0.record(entry.min_rtt_us as f64 / 1000.0);
            unique_addresses.entry(net_key).or_default().insert(key.src);
        }

        histograms.retain(|_, (_, time)| *time + self.timeout >= Instant::now());

        let mut intermediate = String::new();
        if !histograms.is_empty() {
            formatting::write_type_line(
                &mut intermediate,
                &self.bucket_name,
                None,
                None,
                "histogram",
            );
            output.write_all(intermediate.as_bytes()).unwrap();
            intermediate.clear();
        }
        for (key, (histogram, _)) in &histograms {
            let labels = LabelSet::from_key_and_global(
                &Key::from_parts(
                    self.bucket_name.clone(),
                    vec![
                        Label::new("src_network", key.src.addr().to_string()),
                        Label::new(
                            "netmask",
                            if key.src.addr().is_ipv4() {
                                self.v4_netmask.to_string()
                            } else {
                                self.v6_netmask.to_string()
                            },
                        ),
                        Label::new("dst_network", key.dst.addr().to_string()),
                        Label::new("latency_type", key.latency_type.clone()),
                        Label::new("host", key.host.clone()),
                    ],
                ),
                &Default::default(),
            );

            for (le, count) in histogram.buckets() {
                formatting::write_metric_line(
                    &mut intermediate,
                    &self.bucket_name,
                    Some("bucket"),
                    &labels,
                    Some(("le", le)),
                    count,
                    None,
                );
            }
            formatting::write_metric_line(
                &mut intermediate,
                &self.bucket_name,
                Some("bucket"),
                &labels,
                Some(("le", "+Inf")),
                histogram.count(),
                None,
            );

            formatting::write_metric_line::<&str, f64>(
                &mut intermediate,
                &self.bucket_name,
                Some("sum"),
                &labels,
                None,
                histogram.sum(),
                None,
            );
            formatting::write_metric_line::<&str, u64>(
                &mut intermediate,
                &self.bucket_name,
                Some("count"),
                &labels,
                None,
                histogram.count(),
                None,
            );

            // Each set gets its own write invocation.
            output.write_all(intermediate.as_bytes()).unwrap();
            intermediate.clear();

            output.write_all(b"\n").unwrap();
        }

        if !unique_addresses.is_empty() {
            formatting::write_type_line(
                &mut intermediate,
                &self.unique_addresses_name,
                None,
                None,
                "counter",
            );
        }
        for (key, addresses) in unique_addresses {
            let labels = LabelSet::from_key_and_global(
                &Key::from_parts(
                    self.bucket_name.clone(),
                    vec![
                        Label::new("src_network", key.src.addr().to_string()),
                        Label::new(
                            "netmask",
                            if key.src.addr().is_ipv4() {
                                self.v4_netmask.to_string()
                            } else {
                                self.v6_netmask.to_string()
                            },
                        ),
                        Label::new("dst_network", key.dst.addr().to_string()),
                        Label::new("latency_type", key.latency_type.clone()),
                        Label::new("host", key.host.clone()),
                    ],
                ),
                &Default::default(),
            );
            formatting::write_metric_line::<&str, u64>(
                &mut intermediate,
                &self.unique_addresses_name,
                None,
                &labels,
                None,
                addresses.len() as u64,
                None,
            );
            output.write_all(intermediate.as_bytes()).unwrap();
            intermediate.clear();
        }

        self.metrics.swap(histograms.into());
    }
}

#[tonic::async_trait]
impl QuicmopSocketMetricsService for Collector {
    async fn stream_metrics(
        &self,
        request: tonic::Request<tonic::Streaming<AgentMetricsRequest>>,
    ) -> std::result::Result<tonic::Response<CollectorResponse>, tonic::Status> {
        let mut inner = request.into_inner();
        while let Ok(Some(metrics)) = inner.message().await {
            for metric in &metrics.metrics {
                if let Some(item_metrics) = metric.metrics {
                    let src: IpAddr = metric
                        .src
                        .as_ref()
                        .and_then(|i| i.clone().try_into().ok())
                        .unwrap();
                    let dst: IpAddr = metric
                        .dst
                        .as_ref()
                        .and_then(|i| i.clone().try_into().ok())
                        .unwrap();
                    self.addresses
                        .insert(
                            AddressKey {
                                src,
                                dst,
                                latency_type: metric.latency_type.clone(),
                                host: metric.host.clone(),
                            },
                            AddressEntry {
                                min_rtt_us: item_metrics.min_rtt_us,
                                last_update: Instant::now(),
                            },
                        )
                        .await;
                    // histogram!("bucket", "src_network" => src_net.addr().to_string(), "netmask" => if src.is_ipv4() { self.v4_netmask.to_string() } else { self.v6_netmask.to_string() }, "dst_network" => dst_net.addr().to_string(), "latency_type" => metric.latency_type.clone(), "host" => metric.host.clone()).record(item_metrics.min_rtt_us as f64 / 1000.0);
                }
            }
        }
        Ok(tonic::Response::new(CollectorResponse {}))
    }
}
