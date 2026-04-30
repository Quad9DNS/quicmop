use std::net::IpAddr;

use ipnet::IpNet;
use metrics::histogram;
use quicmop_proto::proto::{
    AgentMetricsRequest, CollectorResponse,
    quicmop_socket_metrics_service_server::QuicmopSocketMetricsService,
};
use tracing::info;

pub struct Collector {
    pub v4_netmask: u8,
    pub v6_netmask: u8,
}

#[tonic::async_trait]
impl QuicmopSocketMetricsService for Collector {
    async fn stream_metrics(
        &self,
        request: tonic::Request<tonic::Streaming<AgentMetricsRequest>>,
    ) -> std::result::Result<tonic::Response<CollectorResponse>, tonic::Status> {
        info!("Received request: {:?}", request);
        let mut inner = request.into_inner();
        while let Ok(Some(metrics)) = inner.message().await {
            for metric in &metrics.metrics {
                if let Some(item_metrics) = metric.metrics {
                    let src: IpAddr = metric
                        .src
                        .as_ref()
                        .and_then(|i| i.clone().try_into().ok())
                        .unwrap();
                    let src_net = IpNet::new(
                        src,
                        if src.is_ipv4() {
                            self.v4_netmask
                        } else {
                            self.v6_netmask
                        },
                    )
                    .unwrap()
                    .trunc();
                    let dst: IpAddr = metric
                        .dst
                        .as_ref()
                        .and_then(|i| i.clone().try_into().ok())
                        .unwrap();
                    let dst_net = IpNet::new(
                        dst,
                        if dst.is_ipv4() {
                            self.v4_netmask
                        } else {
                            self.v6_netmask
                        },
                    )
                    .unwrap()
                    .trunc();
                    histogram!("bucket", "src_network" => src_net.addr().to_string(), "netmask" => if src.is_ipv4() { self.v4_netmask.to_string() } else { self.v6_netmask.to_string() }, "dst_network" => dst_net.addr().to_string(), "latency_type" => metric.latency_type.clone(), "host" => metric.host.clone()).record(item_metrics.min_rtt_us as f64 / 1000.0);
                }
            }
            info!("Received metrics: {:?}", metrics);
        }
        Ok(tonic::Response::new(CollectorResponse {}))
    }
}
