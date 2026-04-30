use std::{pin::Pin, time::Duration};

use netlink_packet_core::{
    NLM_F_DUMP, NLM_F_REQUEST, NetlinkHeader, NetlinkMessage, NetlinkPayload,
};
use netlink_packet_sock_diag::{
    AF_INET, AF_INET6, IPPROTO_TCP, SockDiagMessage,
    inet::{ExtensionFlags, InetRequest, SocketId, StateFlags, nlas::Nla},
};
use netlink_sys::{Socket, protocols::NETLINK_SOCK_DIAG};
use quicmop_proto::{Metrics, SocketMetrics, proto::AgentMetricsRequest};
use tokio_stream::Stream;

enum IpVersion {
    V4,
    V6,
}

impl IpVersion {
    fn build_request(&self) -> SockDiagMessage {
        SockDiagMessage::InetRequest(InetRequest {
            family: match self {
                IpVersion::V4 => AF_INET,
                IpVersion::V6 => AF_INET6,
            },
            protocol: IPPROTO_TCP,
            extensions: ExtensionFlags::INFO,
            states: StateFlags::all(),
            socket_id: match self {
                IpVersion::V4 => SocketId::new_v4(),
                IpVersion::V6 => SocketId::new_v6(),
            },
        })
    }
}

struct NetlinkTcpInfoRequest {
    ip_version: IpVersion,
}

impl NetlinkTcpInfoRequest {
    fn new_v4() -> Self {
        Self {
            ip_version: IpVersion::V4,
        }
    }

    fn new_v6() -> Self {
        Self {
            ip_version: IpVersion::V6,
        }
    }

    fn request_on_socket(&self, socket: &Socket) -> Result<usize, Box<dyn std::error::Error>> {
        // Build request
        let mut nl_hdr = NetlinkHeader::default();
        nl_hdr.flags = NLM_F_REQUEST | NLM_F_DUMP;
        let mut nl_msg = NetlinkMessage::new(nl_hdr, self.ip_version.build_request().into());
        nl_msg.finalize();

        // Send request
        let mut buf = vec![0; nl_msg.header.length as usize];
        nl_msg.serialize(&mut buf);
        Ok(socket.send(&buf[..], 0)?)
    }
}

struct NetlinkSingleTypeLoader {
    request: NetlinkTcpInfoRequest,
    latest_activity: u32,
}

impl NetlinkSingleTypeLoader {
    fn new_v4() -> Self {
        Self {
            request: NetlinkTcpInfoRequest::new_v4(),
            latest_activity: 0,
        }
    }

    fn new_v6() -> Self {
        Self {
            request: NetlinkTcpInfoRequest::new_v6(),
            latest_activity: 0,
        }
    }

    pub fn load(
        &mut self,
        socket: &Socket,
    ) -> Result<AgentMetricsRequest, Box<dyn std::error::Error>> {
        self.request.request_on_socket(socket)?;

        // Receive responses
        let mut recv_buf = vec![0; 4096];
        let mut offset = 0;

        let mut request = AgentMetricsRequest {
            metrics: Vec::default(),
        };

        let mut latest = self.latest_activity;

        'outer: while let Ok(size) = socket.recv(&mut &mut recv_buf[..], 0) {
            if size == 0 {
                break;
            }
            loop {
                let bytes = &recv_buf[offset..];
                let packet = <NetlinkMessage<SockDiagMessage>>::deserialize(bytes).unwrap();

                match packet.payload {
                    NetlinkPayload::InnerMessage(SockDiagMessage::InetResponse(resp)) => {
                        for attr in resp.nlas {
                            if let Nla::TcpInfo(info) = attr {
                                let latest_activity = info
                                    .last_data_sent
                                    .max(info.last_data_recv)
                                    .max(info.last_ack_sent)
                                    .max(info.last_ack_recv);
                                if self.latest_activity == 0 {
                                    latest = latest.max(latest_activity);
                                    request.metrics.push(
                                        SocketMetrics {
                                            src: resp.header.socket_id.source_address,
                                            dst: resp.header.socket_id.destination_address,
                                            metrics: Metrics {
                                                min_rtt_us: info.min_rtt as u64,
                                            },
                                            latency_type: "TCP".to_string(),
                                            host: "test".to_string(),
                                        }
                                        .into(),
                                    );
                                    println!(
                                        "Socket {:?}:{:?} => {:?}:{:?} → min RTT = {} µs",
                                        resp.header.socket_id.source_address,
                                        resp.header.socket_id.source_port,
                                        resp.header.socket_id.destination_address,
                                        resp.header.socket_id.destination_port,
                                        info.min_rtt
                                    );
                                } else if latest_activity > self.latest_activity {
                                    println!("Latest activity: {}", latest_activity);
                                    self.latest_activity = latest_activity;
                                    request.metrics.push(
                                        SocketMetrics {
                                            src: resp.header.socket_id.source_address,
                                            dst: resp.header.socket_id.destination_address,
                                            metrics: Metrics {
                                                min_rtt_us: info.min_rtt as u64,
                                            },
                                            latency_type: "TCP".to_string(),
                                            host: "test".to_string(),
                                        }
                                        .into(),
                                    );
                                    println!(
                                        "Socket {:?}:{:?} => {:?}:{:?} → min RTT = {} µs",
                                        resp.header.socket_id.source_address,
                                        resp.header.socket_id.source_port,
                                        resp.header.socket_id.destination_address,
                                        resp.header.socket_id.destination_port,
                                        info.min_rtt
                                    );
                                }
                            }
                        }
                    }

                    NetlinkPayload::Done(_) => {
                        break 'outer;
                    }
                    NetlinkPayload::Error(e) => {
                        eprintln!("Netlink error: {:?}", e);
                        break;
                    }

                    _ => {}
                }

                offset += packet.header.length as usize;

                if offset == size || packet.header.length == 0 {
                    offset = 0;
                    break;
                }
            }
        }

        if self.latest_activity == 0 {
            self.latest_activity = latest;
        }

        Ok(request)
    }
}

pub struct NetlinkLoader {
    v4_loader: NetlinkSingleTypeLoader,
    v6_loader: NetlinkSingleTypeLoader,
    interval: Duration,
}

impl NetlinkLoader {
    pub fn new(interval: Duration) -> Self {
        Self {
            v4_loader: NetlinkSingleTypeLoader::new_v4(),
            v6_loader: NetlinkSingleTypeLoader::new_v6(),
            interval,
        }
    }

    pub fn start_loading(
        mut self,
    ) -> Result<Pin<Box<dyn Stream<Item = AgentMetricsRequest> + Send>>, Box<dyn std::error::Error>>
    {
        // Create NETLINK_SOCK_DIAG socket
        let mut socket = Socket::new(NETLINK_SOCK_DIAG)?;
        socket.bind_auto()?;

        Ok(Box::pin(async_stream::stream! {
            loop {
                let mut request = AgentMetricsRequest {
                    metrics: Vec::default(),
                };

                match self.v4_loader.load(&socket) {
                    Ok(mut req) => {
                        request.metrics.append(&mut req.metrics);
                    }
                    Err(_) => {
                        println!("Error!");
                    }
                }
                match self.v6_loader.load(&socket) {
                    Ok(mut req) => {
                        request.metrics.append(&mut req.metrics);
                    }
                    Err(_) => {
                        println!("Error!");
                    }
                }

                yield request;
                tokio::time::sleep(self.interval).await;
            }
        }))
    }
}
