#![allow(missing_docs)]
use std::io;
use std::process::ExitStatus;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use clap::Parser;
use exitcode::ExitCode;
use futures::{FutureExt, StreamExt};
use metrics::{Unit, describe_gauge, gauge};
use metrics_exporter_prometheus::PrometheusBuilder;
use metrics_util::layers::{PrefixLayer, Stack};
use netobserv_flow_proto::proto::collector_server::CollectorServer;
use quicmop_metrics::metrics::MetricsProcessor;
use quicmop_metrics_exporters::NoopMetricsExtraProvider;
use quicmop_proto::proto::quicmop_socket_metrics_service_client::QuicmopSocketMetricsServiceClient;
use quicmop_signals::{ServiceOsSignals, ServiceSignal};
use tokio::runtime::Runtime;
use tokio::sync::broadcast::{self, Receiver, Sender};
use tokio::task::{JoinHandle, JoinSet};
use tokio::time::interval;
use tokio_stream::wrappers::BroadcastStream;
use tonic::transport::{Endpoint, Server};
use tracing::{Level, error, info, warn};
use tracing_subscriber::fmt::format::FmtSpan;

use crate::adapter::NetobservAdapter;
use crate::cli::CliArgs;
use crate::config::ServiceConfig;

use std::os::unix::process::ExitStatusExt;
use tokio::runtime::Handle;

pub struct Service<T> {
    pub config: ServiceConfig,
    pub state: T,
}

pub struct InitState {
    pub signals: ServiceOsSignals,
    pub metrics_processor: MetricsProcessor,
}

impl Service<()> {
    pub fn init_and_run() -> ExitStatus {
        Service::<InitState>::run()
    }
}

impl Service<InitState> {
    pub fn run() -> ExitStatus {
        let (runtime, app) = Self::prepare_start().unwrap_or_else(|code| std::process::exit(code));

        runtime.block_on(app.run())
    }

    pub fn prepare_start() -> Result<(Runtime, Service<StartedState>), ExitCode> {
        Self::prepare()
            .and_then(|(runtime, app)| app.start(runtime.handle()).map(|app| (runtime, app)))
    }

    pub fn prepare() -> Result<(Runtime, Self), ExitCode> {
        let args = CliArgs::try_parse().map_err(|error| {
            _ = error.print();
            error.exit_code()
        })?;

        Self::prepare_from_config(args.try_into().map_err(|err| {
            // The tracing subscriber is never initialized before this
            tracing_subscriber::fmt()
                .with_file(false)
                .with_target(false)
                .with_max_level(Level::INFO)
                .with_writer(io::stderr)
                .init();
            error!(message = "Configuration error.", error = %err);
            exitcode::USAGE
        })?)
    }

    pub fn prepare_from_config(config: ServiceConfig) -> Result<(Runtime, Self), ExitCode> {
        let init_time = Instant::now();

        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("Building async runtime failed!");

        tracing_subscriber::fmt()
            .with_file(false)
            .with_target(false)
            .with_max_level(config.process.log_level)
            .with_span_events(FmtSpan::FULL)
            .with_writer(io::stderr)
            .init();

        let metrics_recorder = PrometheusBuilder::new().build_recorder();
        let metrics_handle = metrics_recorder.handle();

        let layers = Stack::new(metrics_recorder);

        let metrics_prefix = config.metrics.prefix.clone();
        if !metrics_prefix.is_empty() {
            layers
                .push(PrefixLayer::new(metrics_prefix))
                .install()
                .expect("Failed preparing metrics recorder!");
        } else {
            layers
                .install()
                .expect("Failed preparing metrics recorder!");
        }

        let signals = ServiceOsSignals::new(&runtime);
        let metrics_processor = MetricsProcessor::from_exporters(
            config.metrics.exporters.clone(),
            metrics_handle,
            init_time,
        );
        Ok((
            runtime,
            Self {
                config,
                state: InitState {
                    signals,
                    metrics_processor,
                },
            },
        ))
    }

    pub fn start(self, handle: &Handle) -> Result<Service<StartedState>, ExitCode> {
        let Self {
            config,
            state:
                InitState {
                    signals,
                    metrics_processor,
                },
        } = self;

        let (shutdown_tx, mut shutdown_rx) = tokio::sync::broadcast::channel::<()>(1);

        let metrics_handle = handle.spawn(metrics_processor.run(
            handle.clone(),
            Arc::new(NoopMetricsExtraProvider),
            shutdown_tx.subscribe(),
        ));

        let hostname = config.output.collector_hostname.clone();
        let port = config.output.collector_port;
        let url = format!("grpc://{hostname}:{port}");

        let (metrics_tx, _) = broadcast::channel(4096);

        let adapter = Arc::new(NetobservAdapter::new(
            metrics_tx.clone(),
            config.agent.hostname.clone().clone().unwrap_or_else(|| {
                rustix::system::uname()
                    .nodename()
                    .to_string_lossy()
                    .to_string()
            }),
        ));

        let netobserv_server_handle = handle.spawn(
            Server::builder()
                .add_service(CollectorServer::from_arc(Arc::clone(&adapter)))
                .serve_with_shutdown(config.agent.netobserv_grpc_server_addr, async move {
                    shutdown_rx
                        .recv()
                        .map(|_| info!("Shutting down netobserv gRPC server"))
                        .await
                }),
        );

        let mut client_shutdown_rx = shutdown_tx.subscribe();
        let grpc_client_handle = handle.spawn(async move {
            let endpoint = Endpoint::new(url).unwrap().connect_lazy();
            let mut client = QuicmopSocketMetricsServiceClient::new(endpoint);

            loop {
                let stream =
                    BroadcastStream::new(metrics_tx.subscribe()).filter_map(async |v| v.ok());
                tokio::select! {
                    _ = client_shutdown_rx.recv() => {
                        break;
                    }

                    res = client.stream_metrics(stream).boxed() => {
                        match res {
                            Ok(_) => break,
                            Err(err) => error!("Failed sending data to the collector: {:?}", err),
                        }
                    }
                }
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
            Ok(())
        });

        let mut service_handles = JoinSet::new();
        service_handles.spawn_on(async { grpc_client_handle.await.unwrap() }, handle);

        service_handles.spawn_on(async { netobserv_server_handle.await.unwrap() }, handle);

        Ok(Service {
            config,
            state: StartedState {
                signals,
                metrics_handle,
                service_handles,
                shutdown_tx,
            },
        })
    }
}

pub struct StartedState {
    pub signals: ServiceOsSignals,
    pub metrics_handle: JoinHandle<()>,
    pub service_handles: JoinSet<Result<(), tonic::transport::Error>>,
    pub shutdown_tx: Sender<()>,
}

impl Service<StartedState> {
    pub async fn run(self) -> ExitStatus {
        self.main().await.shutdown().await
    }

    pub async fn main(self) -> Service<FinishedState> {
        let Service {
            config,
            state:
                StartedState {
                    signals,
                    metrics_handle,
                    service_handles,
                    shutdown_tx,
                },
        } = self;

        let mut signal_rx = signals.receiver;

        describe_gauge!(
            "last_reload_signal",
            Unit::Seconds,
            "Timestamp of last reload signal received"
        );
        let last_reload_signal = gauge!("last_reload_signal");

        let signal = loop {
            tokio::select! {
                signal = signal_rx.recv() => {
                    info!(message = "Handling signal", signal = ?signal);
                    match signal{
                        Ok(ServiceSignal::ReloadConfig) => {
                            // Other components will handle config reload
                            // Here we will just emit the metric
                            let now = SystemTime::now();
                            // Ignoring error here, since there is nothing meaningful to be done
                            if let Ok(epoch_timestamp) = now.duration_since(UNIX_EPOCH) {
                                last_reload_signal.set(epoch_timestamp.as_secs_f64())
                            }
                        },
                        Ok(ServiceSignal::Shutdown) => {
                            let _ = shutdown_tx.send(());
                            info!("Starting graceful shutdown. ({} ms)", config.process.shutdown_timeout.as_millis());
                            break ServiceSignal::Shutdown;
                        }
                        Ok(ServiceSignal::Quit) => {
                            break ServiceSignal::Quit;
                        }
                        Err(err) => {
                            error!(message = "Receiving OS signal failed!", error = %err);
                        }
                    }
                }

                else => unreachable!("Signal streams never end"),
            }
        };

        Service {
            config,
            state: FinishedState {
                signal,
                signal_receiver: signal_rx,
                metrics_handle,
                service_handles,
            },
        }
    }
}

pub struct FinishedState {
    pub signal: ServiceSignal,
    pub signal_receiver: Receiver<ServiceSignal>,
    pub metrics_handle: JoinHandle<()>,
    pub service_handles: JoinSet<Result<(), tonic::transport::Error>>,
}

impl Service<FinishedState> {
    pub async fn shutdown(self) -> ExitStatus {
        let Service {
            config,
            state:
                FinishedState {
                    signal,
                    signal_receiver,
                    metrics_handle,
                    service_handles,
                },
        } = self;

        match signal {
            ServiceSignal::Shutdown => {
                Self::stop(config, metrics_handle, service_handles, signal_receiver).await
            }
            ServiceSignal::Quit => Self::quit(),
            _ => unreachable!(),
        }
    }

    async fn stop(
        config: ServiceConfig,
        metrics_handle: JoinHandle<()>,
        mut service_handles: JoinSet<Result<(), tonic::transport::Error>>,
        mut signal_rx: Receiver<ServiceSignal>,
    ) -> ExitStatus {
        let mut graceful_timeout = interval(config.process.shutdown_timeout);
        graceful_timeout.reset();

        if !service_handles.is_empty() && service_handles.try_join_next().is_some() {
            metrics_handle.abort();
            let _ = metrics_handle.await;
            return ExitStatus::from_raw(exitcode::OK);
        }

        tokio::select! {
            _ = service_handles.join_all(), if !service_handles.is_empty() => {
                metrics_handle.abort();
                let _ = metrics_handle.await;
                ExitStatus::from_raw(exitcode::OK)
            }

            _ = graceful_timeout.tick() => {
                warn!("Graceful shutdown timed out. Forcing shutdown.");
                ExitStatus::from_raw(exitcode::OK)
            }

            _ = signal_rx.recv() => Self::quit(),
        }
    }

    fn quit() -> ExitStatus {
        ExitStatus::from_raw(exitcode::OK)
    }
}
