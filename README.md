# quicmop

> The tool is still under active development!

QUIC and TCP latency measurement toolkit. Takes in measurements from different sources, aggregates them and exposes them in Prometheus and other formats.

## Usage

Run a single instance of `quicmop-collector` and run agents on hosts that you want to collect measurements on - point them to the host with collector, using `--collector_hostname`.

## Components

### Collector

Quicmop collector is the central component of quicmop system that aggregates metrics collected by agents, groups them based on configured netmasks and other configuration and exposes metrics in Prometheus and other formats.

### Agents

All agents collect measurements and send them to collector. They also expose metrics of their own, to monitor their performance.

#### Kernel Agent

Kernel agent collects TCP latency measurements periodically, using [netlink sock-diag API](https://www.man7.org/linux/man-pages/man7/sock_diag.7.html) and sending new data to the collector.

#### Netobserv eBPF Agent Adapter

Netobserv eBPF agent adapter acts as an adapter for [Netobserv eBPF agent](github.com/netobserv/netobserv-ebpf-agent). It runs a gRPC server for netobserv eBPF agent to send data to, adds additional metadata to the received data and passes it to the quicmop collector. It is possible to point Netobserv eBPF agent directly to the quicmop collector, but this way it is possible to add additional metadata to metrics collected by it.

#### Qlog Agent

Qlog agent works by reading from quicmop logs from `QLOG_DIR` - each file in that directory should represent qlog of a different connection. Qlog provides limited data about connection source IP, so the server writing qlogs should write source IP addresses into title for qlog agent to work properly.

## Installation

### From source

Install using `make`:
```
$ make
# make install
```

### Containers

Build containers using provided `make` recipes:
```
$ make container-collector-debian
$ make container-collector-alpine
$ make container-kernel-agent-debian
$ make container-kernel-agent-alpine
$ make container-qlog-agent-debian
$ make container-qlog-agent-alpine
$ make container-netobserv-ebpf-agent-adapter-debian
$ make container-netobserv-ebpf-agent-adapter-alpine
```

## Configuration

All components store their default configuration in `/etc/quicmop`. Run `--help` of each component for CLI options. Default configuration files can be [found in the repository](./distribution).

## License

[MIT](./LICENSE)
