#!/bin/bash

# SPDX-FileCopyrightText: © 2025 Phala Network <dstack@phala.network>
#
# SPDX-License-Identifier: Apache-2.0

# Pin APT packages to exact versions from a frozen Debian snapshot.
# Usage: pin-packages.sh <pkg-list-file>
#
# This script:
# 1. Points APT at a frozen snapshot.debian.org mirror (reproducible package sources)
# 2. Reads package=version pairs from the given file and creates APT pin preferences
#    with priority 1001 to force exact versions

set -e

PKG_LIST=$1
SNAPSHOT_DATE=${SNAPSHOT_DATE:-20260317T000000Z}

if [ -z "$PKG_LIST" ]; then
    echo "Usage: $0 <pkg-list-file>" >&2
    exit 1
fi

# Detect base image suite (e.g. bookworm, trixie). Different Debian releases
# ship different sources layouts (legacy sources.list vs deb822
# sources.list.d/*.sources), so we must wipe both and rewrite from scratch
# pointing at the frozen snapshot for this exact suite. Otherwise the base
# image's default live sources stay active and packages drift on every build.
# shellcheck source=/dev/null
DISTRO_ID=$(. /etc/os-release && echo "${ID:-}")
SUITE=$(. /etc/os-release && echo "${VERSION_CODENAME:-}")
if [ -z "$SUITE" ]; then
    echo "could not detect distro suite from /etc/os-release" >&2
    exit 1
fi

rm -f /etc/apt/sources.list
rm -f /etc/apt/sources.list.d/*.list /etc/apt/sources.list.d/*.sources

if [ "$DISTRO_ID" = "ubuntu" ]; then
    # Ubuntu base (26.04 acpi-builder + final oracle stage). Point at the frozen
    # snapshot.ubuntu.com mirror and enable deb-src so `apt-get source qemu` and
    # `apt-get build-dep qemu` resolve (N1). main+universe covers the qemu
    # build-dependency closure. Integrity is GPG-verified via the base image's
    # ubuntu-archive keyring; the base has no ca-certificates yet and the
    # snapshot is served over TLS, so peer verification is disabled here (the
    # signed Release still guarantees content integrity).
    UBUNTU_SNAPSHOT_DATE=${UBUNTU_SNAPSHOT_DATE:-20260701T000000Z}
    SNAP="https://snapshot.ubuntu.com/ubuntu/${UBUNTU_SNAPSHOT_DATE}"
    cat > /etc/apt/sources.list <<EOF
deb [check-valid-until=no] ${SNAP} ${SUITE} main universe
deb [check-valid-until=no] ${SNAP} ${SUITE}-updates main universe
deb [check-valid-until=no] ${SNAP} ${SUITE}-security main universe
deb-src [check-valid-until=no] ${SNAP} ${SUITE} main universe
deb-src [check-valid-until=no] ${SNAP} ${SUITE}-updates main universe
deb-src [check-valid-until=no] ${SNAP} ${SUITE}-security main universe
EOF
    echo 'Acquire::https::Verify-Peer "false";' > /etc/apt/apt.conf.d/20snapshot-no-verify-peer
else
    # Debian base (rust verifier-builder stage; kms/gateway builder twins).
    cat > /etc/apt/sources.list <<EOF
deb [check-valid-until=no] http://snapshot.debian.org/archive/debian/${SNAPSHOT_DATE} ${SUITE} main
deb [check-valid-until=no] http://snapshot.debian.org/archive/debian-security/${SNAPSHOT_DATE} ${SUITE}-security main
EOF
fi
echo 'Acquire::Check-Valid-Until "false";' > /etc/apt/apt.conf.d/10no-check-valid-until

mkdir -p /etc/apt/preferences.d
while IFS= read -r line; do
    pkg=$(echo "$line" | cut -d= -f1)
    ver=$(echo "$line" | cut -d= -f2)
    if [ -n "$pkg" ] && [ -n "$ver" ]; then
        printf 'Package: %s\nPin: version %s\nPin-Priority: 1001\n\n' "$pkg" "$ver" >> /etc/apt/preferences.d/pinned-packages
    fi
done < "$PKG_LIST"
