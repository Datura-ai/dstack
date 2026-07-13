#!/bin/bash

# SPDX-FileCopyrightText: © 2025 Phala Network <dstack@phala.network>
#
# SPDX-License-Identifier: Apache-2.0

BUILD_DIR="$1"
PREFIX="$2"
if [ -z "$BUILD_DIR" ]; then
  echo "Usage: $0 <build-directory>"
  exit 1
fi

mkdir -p "$BUILD_DIR"
cd "$BUILD_DIR"

# Pick a deterministic SOURCE_DATE_EPOCH for the QEMU build. The source tree can
# reach us two ways:
#   - `apt-get source qemu` (Canonical 10.2.1 oracle): a dpkg source tree with a
#     debian/changelog but NO git history -> use the changelog timestamp.
#   - `git clone kvinwang/qemu-tdx` (9.2.1 oracle / kms twin): upstream QEMU git
#     with no debian/ dir -> use the last commit timestamp.
# Falling through keeps whatever SOURCE_DATE_EPOCH the caller already exported
# (build-lib.sh passes the dstack commit timestamp as a build-arg).
if [ -f ../debian/changelog ]; then
  export SOURCE_DATE_EPOCH=$(cd .. && dpkg-parsechangelog -S timestamp)
elif git -C .. rev-parse --git-dir >/dev/null 2>&1; then
  export SOURCE_DATE_EPOCH=$(git -C .. log -1 --pretty=%ct)
fi
export CFLAGS="-DDUMP_ACPI_TABLES -Wno-builtin-macro-redefined -D__DATE__=\"\" -D__TIME__=\"\" -D__TIMESTAMP__=\"\""
export LDFLAGS="-Wl,--build-id=none"

# -Dinstall_blobs=false: the oracle only extracts the qemu-system-x86_64 binary,
# it never `make install`s the firmware blobs. Canonical's `+ds` source tarball
# strips many prebuilt blobs (e.g. ast27x0_bootrom.bin), which would otherwise
# fail meson's install_data() existence check at configure time. Harmless for the
# git-clone (kvinwang) oracle path too. configure forwards -D* straight to meson.
../configure \
  --prefix="$PREFIX" \
  --target-list=x86_64-softmmu \
  --disable-werror \
  -Dinstall_blobs=false

echo ""
echo "Build configured for reproducibility in $BUILD_DIR"
echo "To build, run: cd $BUILD_DIR && make"
