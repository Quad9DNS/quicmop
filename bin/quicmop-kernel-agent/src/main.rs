mod netlink_loader;

use std::{env::args, time::Duration};

use quicmop_proto::proto::quicmop_socket_metrics_service_client::QuicmopSocketMetricsServiceClient;

use crate::netlink_loader::NetlinkLoader;

// TODO: metrics, proper args, etc.
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut osargs = args();
    osargs.next();
    let url = osargs.next().unwrap_or("grpc://localhost:8765".to_string());

    let mut client = QuicmopSocketMetricsServiceClient::connect(url)
        .await
        .unwrap();

    let requests_stream = NetlinkLoader::new(
        Duration::from_secs(5),
        rustix::system::uname()
            .nodename()
            .to_string_lossy()
            .to_string(),
    )
    .start_loading()
    .unwrap();

    client.stream_metrics(requests_stream).await.unwrap();
    Ok(())
}
