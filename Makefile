.POSIX:
.SUFFIXES:
.SUFFIXES: .1 .5 .1.scd .5.scd

PREFIX?=/usr/local
BINDIR?=$(PREFIX)/bin
MANDIR?=$(PREFIX)/share/man

VERSION?=$(shell cat Cargo.toml | grep version | cut -f 3 -d " " | cut -f2 -d '"' | head -n 1 || echo unknown)

# Override the container tool. Tries docker first and then tries podman.
export CONTAINER_TOOL ?= auto
ifeq ($(CONTAINER_TOOL),auto)
	ifeq ($(shell docker version >/dev/null 2>&1 && echo docker), docker)
		override CONTAINER_TOOL = docker
	else ifeq ($(shell podman version >/dev/null 2>&1 && echo podman), podman)
		override CONTAINER_TOOL = podman
	else
		override CONTAINER_TOOL = unknown
	endif
endif

DOCS := $(addprefix target/man/,\
	quicmop-collector.1)

all: doc target/default/release/quicmop-collector target/default/release/quicmop-kernel-agent target/default/release/quicmop-netobserv-ebpf-agent-adapter target/default/release/quicmop-qlog-agent
	cp target/default/release/quicmop-collector target/quicmop-collector
	cp target/default/release/quicmop-kernel-agent target/quicmop-kernel-agent
	cp target/default/release/quicmop-netobserv-ebpf-agent-adapter target/quicmop-netobserv-ebpf-agent-adapter
	cp target/default/release/quicmop-qlog-agent target/quicmop-qlog-agent

.PHONY: container-collector-debian
container-collector-debian: collector-deb
	$(CONTAINER_TOOL) build --build-arg CARGO_BUILD_TARGET="x86_64-unknown-linux-gnu" -f distribution/container/Containerfile-collector.debian .

.PHONY: container-collector-alpine
container-collector-alpine:
	CARGO_TARGET_DIR="target/default" CARGO_BUILD_TARGET="x86_64-unknown-linux-musl" cargo build --release -p quicmop-collector
	$(CONTAINER_TOOL) build --build-arg CARGO_TARGET_DIR="target/default" --build-arg CARGO_BUILD_TARGET="x86_64-unknown-linux-musl" -f distribution/container/Containerfile-collector.alpine .

.PHONY: container-kernel-agent-debian
container-kernel-agent-debian: kernel-agent-deb
	$(CONTAINER_TOOL) build --build-arg CARGO_BUILD_TARGET="x86_64-unknown-linux-gnu" -f distribution/container/Containerfile-kernel-agent.debian .

.PHONY: container-kernel-agent-alpine
container-kernel-agent-alpine:
	CARGO_TARGET_DIR="target/default" CARGO_BUILD_TARGET="x86_64-unknown-linux-musl" cargo build --release -p quicmop-kernel-agent
	$(CONTAINER_TOOL) build --build-arg CARGO_TARGET_DIR="target/default" --build-arg CARGO_BUILD_TARGET="x86_64-unknown-linux-musl" -f distribution/container/Containerfile-kernel-agent.alpine .

.PHONY: container-qlog-agent-debian
container-qlog-agent-debian: qlog-agent-deb
	$(CONTAINER_TOOL) build --build-arg CARGO_BUILD_TARGET="x86_64-unknown-linux-gnu" -f distribution/container/Containerfile-qlog-agent.debian .

.PHONY: container-qlog-agent-alpine
container-qlog-agent-alpine:
	CARGO_TARGET_DIR="target/default" CARGO_BUILD_TARGET="x86_64-unknown-linux-musl" cargo build --release -p quicmop-qlog-agent
	$(CONTAINER_TOOL) build --build-arg CARGO_TARGET_DIR="target/default" --build-arg CARGO_BUILD_TARGET="x86_64-unknown-linux-musl" -f distribution/container/Containerfile-qlog-agent.alpine .

.PHONY: container-netobserv-ebpf-agent-adapter-debian
container-netobserv-ebpf-agent-adapter-debian: netobserv-ebpf-agent-adapter-deb
	$(CONTAINER_TOOL) build --build-arg CARGO_BUILD_TARGET="x86_64-unknown-linux-gnu" -f distribution/container/Containerfile-netobserv-ebpf-agent-adapter.debian .

.PHONY: container-netobserv-ebpf-agent-adapter-alpine
container-netobserv-ebpf-agent-adapter-alpine:
	CARGO_TARGET_DIR="target/default" CARGO_BUILD_TARGET="x86_64-unknown-linux-musl" cargo build --release -p quicmop-netobserv-ebpf-agent-adapter
	$(CONTAINER_TOOL) build --build-arg CARGO_TARGET_DIR="target/default" --build-arg CARGO_BUILD_TARGET="x86_64-unknown-linux-musl" -f distribution/container/Containerfile-netobserv-ebpf-agent-adapter.alpine .

target/%/release/quicmop-collector:
	CARGO_TARGET_DIR="target/$*" cargo build -p quicmop-collector --release

target/%/release/quicmop-kernel-agent:
	CARGO_TARGET_DIR="target/$*" cargo build -p quicmop-kernel-agent --release

target/%/release/quicmop-netobserv-ebpf-agent-adapter:
	CARGO_TARGET_DIR="target/$*" cargo build -p quicmop-netobserv-ebpf-agent-adapter --release

target/%/release/quicmop-qlog-agent:
	CARGO_TARGET_DIR="target/$*" cargo build -p quicmop-qlog-agent --release

.PHONY: collector-deb
collector-deb: doc
	cross build --target x86_64-unknown-linux-gnu -p quicmop-collector --release
	cp "target/x86_64-unknown-linux-gnu/release/quicmop-collector" "target/release/quicmop-collector"
	cargo deb --no-build --variant default --target x86_64-unknown-linux-gnu -p quicmop-collector

.PHONY: collector-rpm
collector-rpm: target/default/generate-rpm/quicmop-collector_$(VERSION)-1.x86_64.rpm doc

target/%/generate-rpm/quicmop-collector_$(VERSION)-1.x86_64.rpm: target/%/release/quicmop-collector $(DOCS)
	cargo generate-rpm -p quicmop-collector --target-dir "target/$*" --variant $*

.PHONY: kernel-agent-deb
kernel-agent-deb: doc
	cross build --target x86_64-unknown-linux-gnu -p quicmop-kernel-agent --release
	cp "target/x86_64-unknown-linux-gnu/release/quicmop-kernel-agent" "target/release/quicmop-kernel-agent"
	cargo deb --no-build --variant default --target x86_64-unknown-linux-gnu -p quicmop-kernel-agent

.PHONY: kernel-agent-rpm
kernel-agent-rpm: target/default/generate-rpm/quicmop-kernel-agent_$(VERSION)-1.x86_64.rpm doc

target/%/generate-rpm/quicmop-kernel-agent_$(VERSION)-1.x86_64.rpm: target/%/release/quicmop-kernel-agent $(DOCS)
	cargo generate-rpm -p quicmop-kernel-agent --target-dir "target/$*" --variant $*

.PHONY: netobserv-ebpf-agent-adapter-deb
netobserv-ebpf-agent-adapter-deb: doc
	cross build --target x86_64-unknown-linux-gnu -p quicmop-netobserv-ebpf-agent-adapter --release
	cp "target/x86_64-unknown-linux-gnu/release/quicmop-netobserv-ebpf-agent-adapter" "target/release/quicmop-netobserv-ebpf-agent-adapter"
	cargo deb --no-build --variant default --target x86_64-unknown-linux-gnu -p quicmop-netobserv-ebpf-agent-adapter

.PHONY: netobserv-ebpf-agent-adapter-rpm
netobserv-ebpf-agent-adapter-rpm: target/default/generate-rpm/quicmop-netobserv-ebpf-agent-adapter_$(VERSION)-1.x86_64.rpm doc

target/%/generate-rpm/quicmop-netobserv-ebpf-agent-adapter_$(VERSION)-1.x86_64.rpm: target/%/release/quicmop-netobserv-ebpf-agent-adapter $(DOCS)
	cargo generate-rpm -p quicmop-netobserv-ebpf-agent-adapter --target-dir "target/$*" --variant $*

.PHONY: qlog-agent-deb
qlog-agent-deb: doc
	cross build --target x86_64-unknown-linux-gnu -p quicmop-qlog-agent --release
	cp "target/x86_64-unknown-linux-gnu/release/quicmop-qlog-agent" "target/release/quicmop-qlog-agent"
	cargo deb --no-build --variant default --target x86_64-unknown-linux-gnu -p quicmop-qlog-agent

.PHONY: qlog-agent-rpm
qlog-agent-rpm: target/default/generate-rpm/quicmop-qlog-agent_$(VERSION)-1.x86_64.rpm doc

target/%/generate-rpm/quicmop-qlog-agent_$(VERSION)-1.x86_64.rpm: target/%/release/quicmop-qlog-agent $(DOCS)
	cargo generate-rpm -p quicmop-qlog-agent --target-dir "target/$*" --variant $*

.PHONY: dev
dev:
	cargo build

.PHONY: fmt
fmt:
	cargo fmt

.PHONY: fmt-check
fmt-check:
	cargo fmt --check

.PHONY: lint
lint:
	cargo clippy

.PHONY: check
check:
	cargo check
	cargo check --no-default-features

.PHONY: check-all
check-all: check lint test check-deny fmt-check

.PHONY: check-deny
check-deny:
	cargo deny check

.PHONY: test
test:
	cargo test

target/man/%.1: doc/man/%.1.scd
	@mkdir -p target/man
	scdoc < $? > $@

target/man/%.5: doc/man/%.5.scd
	@mkdir -p target/man
	scdoc < $? > $@

target/man/%.7: doc/man/%.7.scd
	@mkdir -p target/man
	scdoc < $? > $@

doc: $(DOCS)

# Exists in GNUMake but not in NetBSD make and others.
RM?=rm -f

clean:
	cargo clean

install: $(DOCS)
	mkdir -m755 -p $(DESTDIR)$(BINDIR) $(DESTDIR)$(MANDIR)/man1 $(DESTDIR)$(MANDIR)/man5 $(DESTDIR)$(MANDIR)/man7
	install -m755 target/quicmop-collector $(DESTDIR)$(BINDIR)/quicmop-collector
	install -m755 target/quicmop-kernel-agent $(DESTDIR)$(BINDIR)/quicmop-kernel-agent
	install -m755 target/quicmop-netobserv-ebpf-agent-adapter $(DESTDIR)$(BINDIR)/quicmop-netobserv-ebpf-agent-adapter
	install -m755 target/quicmop-qlog-agent $(DESTDIR)$(BINDIR)/quicmop-qlog-agent

RMDIR_IF_EMPTY:=sh -c '! [ -d $$0 ] || ls -1qA $$0 | grep -q . || rmdir $$0'

uninstall:
	$(RM) $(DESTDIR)$(BINDIR)/quicmop-collector
	$(RM) $(DESTDIR)$(BINDIR)/quicmop-kernel-agent
	$(RM) $(DESTDIR)$(BINDIR)/quicmop-netobserv-ebpf-agent-adapter
	$(RM) $(DESTDIR)$(BINDIR)/quicmop-qlog-agent
	${RMDIR_IF_EMPTY} $(DESTDIR)$(BINDIR)
	$(RMDIR_IF_EMPTY) $(DESTDIR)$(MANDIR)/man1
	$(RMDIR_IF_EMPTY) $(DESTDIR)$(MANDIR)/man5
	$(RMDIR_IF_EMPTY) $(DESTDIR)$(MANDIR)/man7
	$(RMDIR_IF_EMPTY) $(DESTDIR)$(MANDIR)

.PHONY: all doc clean install uninstall
