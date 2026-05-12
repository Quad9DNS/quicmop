use std::{
    collections::{HashMap, HashSet},
    io,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use ipnet::IpNet;
use metrics::{Key, Label, Unit, counter, describe_counter, describe_gauge, gauge};
use metrics_exporter_prometheus::{LabelSet, formatting};
use metrics_util::storage::Histogram;
use moka::future::Cache;
use netobserv_flow_proto::proto::{CollectorReply, Direction, Records};
use quicmop_metrics_exporters::MetricsExtraProvider;
use quicmop_proto::proto::{
    AgentMetricsRequest, CollectorResponse,
    quicmop_socket_metrics_service_server::QuicmopSocketMetricsService,
};

const IPPROTO_TCP: u32 = 6;

#[derive(Clone)]
struct AddressEntry {
    min_rtt_us: u64,
}

#[derive(Clone, Hash, Eq, PartialEq)]
struct AddressKey {
    src: IpAddr,
    dst: IpAddr,
    latency_type: String,
    host: String,
    agent_addr: IpAddr,
    event_time: Instant,
}

#[derive(Clone, Hash, Eq, PartialEq)]
struct NetworkKey {
    src: IpNet,
    dst: IpNet,
    latency_type: String,
    host: String,
    agent_addr: IpAddr,
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
    v4_src_netmask: u8,
    v6_src_netmask: u8,
    v4_dst_netmask: u8,
    v6_dst_netmask: u8,
    buckets: Vec<f64>,
    addresses: Cache<AddressKey, AddressEntry>,
    timeout: Duration,
    bucket_name: String,
    unique_addresses_name: String,
}

impl Collector {
    pub fn new(
        v4_src_netmask: u8,
        v6_src_netmask: u8,
        v4_dst_netmask: u8,
        v6_dst_netmask: u8,
        buckets: Vec<f64>,
        timeout: Duration,
        name_prefix: String,
    ) -> Self {
        describe_gauge!(
            "last_agent_update",
            Unit::Seconds,
            "Timestamp of last update received from an agent"
        );
        describe_counter!(
            "agent_events_received",
            Unit::Count,
            "Number of metrics events received from an agent"
        );
        Self {
            v4_src_netmask,
            v6_src_netmask,
            v4_dst_netmask,
            v6_dst_netmask,
            buckets,
            addresses: Cache::builder()
                .weigher(|k: &AddressKey, _| -> u32 { k.size() + size_of::<AddressEntry>() as u32 })
                .max_capacity(32 * 1024 * 1024) // 32 MiB
                .time_to_live(timeout)
                .build(),
            timeout,
            bucket_name: format!("{name_prefix}_bucket"),
            unique_addresses_name: format!("{name_prefix}_unique_addresses"),
        }
    }
}

impl MetricsExtraProvider for Collector {
    fn render_to_write(&self, output: &mut impl io::Write) {
        let mut histograms = HashMap::new();

        let mut unique_addresses: HashMap<IpNet, HashSet<IpAddr>> = HashMap::new();

        for (key, entry) in self.addresses.iter() {
            let src_net = IpNet::new(
                key.src,
                if key.src.is_ipv4() {
                    self.v4_src_netmask
                } else {
                    self.v6_src_netmask
                },
            )
            .unwrap()
            .trunc();
            let dst_net = IpNet::new(
                key.dst,
                if key.dst.is_ipv4() {
                    self.v4_dst_netmask
                } else {
                    self.v6_dst_netmask
                },
            )
            .unwrap()
            .trunc();
            let net_key = NetworkKey {
                src: src_net,
                dst: dst_net,
                latency_type: key.latency_type.clone(),
                host: key.host.clone(),
                agent_addr: key.agent_addr,
            };
            let x = histograms
                .entry(net_key.clone())
                .or_insert(Histogram::new(&self.buckets).unwrap());
            x.record(entry.min_rtt_us as f64 / 1000.0);
            unique_addresses.entry(src_net).or_default().insert(key.src);
        }

        let mut intermediate = String::new();
        if !histograms.is_empty() {
            formatting::write_help_line(
                &mut intermediate,
                &self.bucket_name,
                None,
                None,
                "minimum roundtrip time per connection in milliseconds",
            );
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
        for (key, histogram) in &histograms {
            let labels = LabelSet::from_key_and_global(
                &Key::from_parts(
                    self.bucket_name.clone(),
                    vec![
                        Label::new("src_network", key.src.addr().to_string()),
                        Label::new(
                            "src_netmask",
                            if key.src.addr().is_ipv4() {
                                self.v4_src_netmask.to_string()
                            } else {
                                self.v6_src_netmask.to_string()
                            },
                        ),
                        Label::new("dst_network", key.dst.addr().to_string()),
                        Label::new(
                            "dst_netmask",
                            if key.dst.addr().is_ipv4() {
                                self.v4_dst_netmask.to_string()
                            } else {
                                self.v6_dst_netmask.to_string()
                            },
                        ),
                        Label::new("latency_type", key.latency_type.clone()),
                        Label::new("host", key.host.clone()),
                        Label::new("agent_addr", key.agent_addr.to_string()),
                    ],
                ),
                &Default::default(),
            );

            for (le, count) in histogram.buckets() {
                formatting::write_metric_line(
                    &mut intermediate,
                    &self.bucket_name,
                    None,
                    &labels,
                    Some(("le", le)),
                    count,
                    None,
                );
            }
            formatting::write_metric_line(
                &mut intermediate,
                &self.bucket_name,
                None,
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
                Some(Unit::Count),
            );

            // Each set gets its own write invocation.
            output.write_all(intermediate.as_bytes()).unwrap();
            intermediate.clear();

            output.write_all(b"\n").unwrap();
        }

        if !unique_addresses.is_empty() {
            formatting::write_help_line(
                &mut intermediate,
                &self.unique_addresses_name,
                Some(Unit::Count),
                None,
                "number of unique addresses observed from a network",
            );
            formatting::write_type_line(
                &mut intermediate,
                &self.unique_addresses_name,
                Some(Unit::Count),
                None,
                "counter",
            );
        }
        for (key, addresses) in unique_addresses {
            let labels = LabelSet::from_key_and_global(
                &Key::from_parts(
                    self.bucket_name.clone(),
                    vec![
                        Label::new("network", key.addr().to_string()),
                        Label::new(
                            "netmask",
                            if key.addr().is_ipv4() {
                                self.v4_src_netmask.to_string()
                            } else {
                                self.v6_src_netmask.to_string()
                            },
                        ),
                        Label::new("timer", self.timeout.as_secs_f64().to_string()),
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
                Some(Unit::Count),
            );
            output.write_all(intermediate.as_bytes()).unwrap();
            intermediate.clear();
        }
    }
}

#[tonic::async_trait]
impl QuicmopSocketMetricsService for Collector {
    async fn stream_metrics(
        &self,
        request: tonic::Request<tonic::Streaming<AgentMetricsRequest>>,
    ) -> std::result::Result<tonic::Response<CollectorResponse>, tonic::Status> {
        let agent_addr = request
            .remote_addr()
            .unwrap_or_else(|| SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0));
        let mut inner = request.into_inner();
        while let Ok(Some(metrics)) = inner.message().await {
            if let Some(metric) = metrics.metrics.first() {
                let now = SystemTime::now();
                if let Ok(epoch_timestamp) = now.duration_since(UNIX_EPOCH) {
                    gauge!("last_agent_update", "host" => metric.host.clone(), "agent_addr" => agent_addr.ip().to_string())
                        .set(epoch_timestamp.as_secs_f64());
                }
            }
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
                        .entry(AddressKey {
                            src,
                            dst,
                            latency_type: metric.latency_type.clone(),
                            host: metric.host.clone(),
                            agent_addr: agent_addr.ip(),
                            event_time: Instant::now(),
                        })
                        .or_insert_with_if(
                            async {
                                AddressEntry {
                                    min_rtt_us: item_metrics.min_rtt_us,
                                }
                            },
                            |v| item_metrics.min_rtt_us < v.min_rtt_us,
                        )
                        .await;
                    counter!("agent_events_received", "host" => metric.host.clone(), "latency_type" => metric.latency_type.clone(), "agent_addr" => agent_addr.ip().to_string()).increment(1);
                }
            }
        }
        Ok(tonic::Response::new(CollectorResponse {}))
    }
}

#[tonic::async_trait]
impl netobserv_flow_proto::proto::collector_server::Collector for Collector {
    async fn send(
        &self,
        request: tonic::Request<Records>,
    ) -> Result<tonic::Response<CollectorReply>, tonic::Status> {
        let agent_addr = request
            .remote_addr()
            .unwrap_or_else(|| SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0));
        let inner = request.into_inner();
        let mut updated_metric = false;
        for entry in inner.entries.iter().filter(|e| {
            e.direction() == Direction::Ingress
                && e.transport
                    .map(|t| t.protocol == IPPROTO_TCP)
                    .unwrap_or(false)
        }) {
            if let Some(rtt) = entry.time_flow_rtt
                && let Some(network) = &entry.network
            {
                let rtt = Duration::from_secs(rtt.seconds as u64)
                    + Duration::from_nanos(rtt.nanos as u64);
                let src: IpAddr = network
                    .src_addr
                    .as_ref()
                    .and_then(|i| i.clone().try_into().ok())
                    .unwrap();
                let dst: IpAddr = network
                    .dst_addr
                    .as_ref()
                    .and_then(|i| i.clone().try_into().ok())
                    .unwrap();
                self.addresses
                    .entry(AddressKey {
                        src,
                        dst,
                        latency_type: "TCP".to_string(),
                        host: entry
                            .agent_ip
                            .as_ref()
                            .and_then(|i| IpAddr::try_from(i.clone()).ok())
                            .map(|i| i.to_string())
                            .unwrap_or_default(),
                        agent_addr: entry
                            .agent_ip
                            .as_ref()
                            .and_then(|i| IpAddr::try_from(i.clone()).ok())
                            .unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED)),
                        event_time: Instant::now(),
                    })
                    .or_insert_with_if(
                        async {
                            AddressEntry {
                                min_rtt_us: rtt.as_micros() as u64,
                            }
                        },
                        |v| (rtt.as_micros() as u64) < v.min_rtt_us,
                    )
                    .await;
                let host = entry
                    .agent_ip
                    .as_ref()
                    .and_then(|i| IpAddr::try_from(i.clone()).ok())
                    .map(|i| i.to_string())
                    .unwrap_or_default();
                if !updated_metric {
                    let now = SystemTime::now();
                    if let Ok(epoch_timestamp) = now.duration_since(UNIX_EPOCH) {
                        gauge!("last_agent_update", "host" => host.clone(), "agent_addr" => agent_addr.to_string())
                            .set(epoch_timestamp.as_secs_f64());
                    }
                    updated_metric = true;
                }
                counter!("agent_events_received", "host" => host.clone(), "latency_type" => "TCP".to_string(), "agent_addr" => agent_addr.to_string()).increment(1);
            }
        }
        Ok(tonic::Response::new(CollectorReply {}))
    }
}
