use std::{net::IpAddr, time::Duration};

use netobserv_flow_proto::proto::{
    CollectorReply, Direction, Records, collector_server::Collector,
};
use quicmop_proto::proto::{AgentMetricsRequest, Metrics, SocketMetricsGroup};
use tokio::sync::broadcast::Sender;

const IPPROTO_TCP: u32 = 6;

pub struct NetobservAdapter {
    metrics_tx: Sender<AgentMetricsRequest>,
    hostname: String,
}

impl NetobservAdapter {
    pub fn new(metrics_tx: Sender<AgentMetricsRequest>, hostname: String) -> Self {
        Self {
            metrics_tx,
            hostname,
        }
    }
}

#[tonic::async_trait]
impl Collector for NetobservAdapter {
    async fn send(
        &self,
        request: tonic::Request<Records>,
    ) -> Result<tonic::Response<CollectorReply>, tonic::Status> {
        let inner = request.into_inner();
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
                self.metrics_tx
                    .send(AgentMetricsRequest {
                        metrics: vec![SocketMetricsGroup {
                            src: Some(src.into()),
                            dst: Some(dst.into()),
                            host: self.hostname.clone(),
                            latency_type: "TCP".to_string(),
                            metrics: Some(Metrics {
                                min_rtt_us: rtt.as_micros() as u64,
                            }),
                        }],
                    })
                    .unwrap();
            }
        }
        Ok(tonic::Response::new(CollectorReply {}))
    }
}
