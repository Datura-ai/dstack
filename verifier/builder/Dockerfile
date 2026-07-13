# SPDX-FileCopyrightText: © 2025 Phala Network <dstack@phala.network>
#
# SPDX-License-Identifier: Apache-2.0

FROM rust:1.92.0-bookworm@sha256:e90e846de4124376164ddfbaab4b0774c7bdeef5e738866295e5a90a34a307a2 AS verifier-builder
COPY --from=build-shared pin-packages.sh /build/
COPY builder/shared/*-pinned-packages.txt /build/
ARG DSTACK_REV
ARG DSTACK_SRC_URL=https://github.com/Dstack-TEE/dstack.git
WORKDIR /build
RUN ./pin-packages.sh ./builder-pinned-packages.txt
RUN apt-get update && \
    apt-get install -y --no-install-recommends \
    git \
    build-essential \
    musl-tools \
    libssl-dev \
    protobuf-compiler \
    libprotobuf-dev \
    clang \
    libclang-dev \
    pkg-config \
    ca-certificates \
    curl && \
    rm -rf /var/lib/apt/lists/* /var/log/* /var/cache/ldconfig/aux-cache
RUN git clone ${DSTACK_SRC_URL} dstack && \
    cd dstack && \
    git checkout ${DSTACK_REV}
RUN rustup target add x86_64-unknown-linux-musl
RUN cd dstack && cargo build --release -p dstack-verifier --target x86_64-unknown-linux-musl
RUN echo "${DSTACK_REV}" > /build/.GIT_REV

# dstack-acpi-tables oracle, built from official Canonical Ubuntu 26.04 QEMU
# source (NOT the kvinwang personal fork). ubuntu:26.04 so the oracle's glibc/ABI
# matches the TDX host that runs the same apt QEMU package.
FROM ubuntu:26.04@sha256:b7f48194d4d8b763a478a621cdc81c27be222ba2206ca3ca6bc42b49685f3d9e AS acpi-builder
COPY --from=build-shared pin-packages.sh config-qemu.sh /build/
COPY builder/shared/qemu-pinned-packages.txt /build/
WORKDIR /build
# Canonical Ubuntu 26.04 LTS main-archive QEMU. HARD GATE (N3): the oracle binary
# MUST be built from the byte-identical package version the TDX host installs.
# A security bump of this pin (…-1ubuntu3.1 -> …3.2) is a measurement re-baseline
# trigger — bump host + oracle together and re-run the byte-equality proof.
ARG QEMU_SRC_VERSION=1:10.2.1+ds-1ubuntu3.1
# pin-packages.sh points apt at the frozen snapshot.ubuntu.com mirror with deb-src
# enabled (N1) so `apt-get source qemu` resolves; the explicit, snapshot-pinned dep
# set below is the proven dstack-oracle minimal set (mirrors the 9.2.1 build) + meson.
# NOTE: `apt-get build-dep qemu` is intentionally NOT used — it drags in the full
# cross-compilation toolchain (arm/hppa/i686 gcc) for QEMU's other targets/firmware,
# none of which a `--target-list=x86_64-softmmu` headless dump build needs. The
# closure stays fully pinned via pin-packages.sh, so reproducibility is unchanged.
# qemu-system-data + ipxe-qemu supply the option ROMs byte-identically to the host.
RUN ./pin-packages.sh ./qemu-pinned-packages.txt && \
    apt-get update && \
    apt-get install -y --no-install-recommends \
    ca-certificates \
    dpkg-dev \
    patch \
    libslirp-dev \
    libglib2.0-dev \
    libpixman-1-dev \
    libfdt-dev \
    python3-pip \
    python3-venv \
    meson \
    ninja-build \
    pkg-config \
    python3-sphinx \
    python3-sphinx-rtd-theme \
    build-essential \
    flex \
    bison \
    qemu-system-data \
    ipxe-qemu
# Patch copied after the apt layer so patch iterations reuse the cached toolchain.
COPY builder/patches /build/patches
# Fetch Canonical QEMU source, apply the offline-dump patch, build the oracle.
# The oracle emits etc/acpi/tables to stdout; rsdp + table-loader are re-derived
# in Rust (dstack-mr/src/acpi.rs::build_tables).
RUN apt-get source "qemu=${QEMU_SRC_VERSION}" && \
    cd qemu-10.2.1+ds && \
    patch -p1 < /build/patches/0001-dump-acpi-tables.patch && \
    ../config-qemu.sh ./build /usr/local && \
    ninja -C build qemu-system-x86_64 && \
    strip build/qemu-system-x86_64 && \
    install -m 755 build/qemu-system-x86_64 /usr/local/bin/dstack-acpi-tables && \
    install -d /usr/local/share/qemu && \
    install -m 644 /usr/share/qemu/efi-virtio.rom /usr/local/share/qemu/ && \
    install -m 644 /usr/share/qemu/kvmvapic.bin /usr/local/share/qemu/ && \
    install -m 644 /usr/share/qemu/linuxboot_dma.bin /usr/local/share/qemu/ && \
    cd .. && rm -rf qemu-10.2.1+ds && \
    rm -rf /var/lib/apt/lists/* /var/log/* /var/cache/ldconfig/aux-cache

# Final image on ubuntu:26.04 so the ubuntu-built dstack-acpi-tables oracle's
# glibc/ABI is satisfied (the musl-static dstack-verifier is base-agnostic).
FROM ubuntu:26.04@sha256:b7f48194d4d8b763a478a621cdc81c27be222ba2206ca3ca6bc42b49685f3d9e
COPY --from=build-shared pin-packages.sh /build/
COPY builder/shared/pinned-packages.txt /build/
WORKDIR /build
RUN ./pin-packages.sh ./pinned-packages.txt && \
    apt-get update && \
    apt-get install -y --no-install-recommends \
    ca-certificates \
    curl \
    libglib2.0-0t64 \
    libslirp0 \
    libpixman-1-0 \
    libfdt1 \
    && rm -rf /var/lib/apt/lists/* /var/log/* /var/cache/ldconfig/aux-cache
COPY --from=verifier-builder /build/dstack/target/x86_64-unknown-linux-musl/release/dstack-verifier /usr/local/bin/dstack-verifier
COPY --from=verifier-builder /build/.GIT_REV /etc/
COPY --from=acpi-builder /usr/local/bin/dstack-acpi-tables /usr/local/bin/dstack-acpi-tables
COPY --from=acpi-builder /usr/local/share/qemu /usr/local/share/qemu
RUN mkdir -p /etc/dstack
COPY dstack-verifier.toml /etc/dstack/dstack-verifier.toml
WORKDIR /var/lib/dstack-verifier
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/dstack-verifier"]
CMD ["--config", "/etc/dstack/dstack-verifier.toml"]
