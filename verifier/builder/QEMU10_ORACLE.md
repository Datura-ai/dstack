# dstack-acpi-tables oracle on Canonical QEMU 10.2.1 — rebuild record + host runbook

Phase-0 spike: rebuild the TDX attestation ACPI oracle from **official Canonical
Ubuntu 26.04 LTS QEMU** instead of the `kvinwang/qemu-tdx` personal fork, keeping
full ACPI-content verification (`acpi_tables_verified=true`), and prove on a
dedicated TDX host that the verifier returns `is_valid=true` on a real 10.2.1 quote.

- Target: `verifier-v0.5.11` (`245201be`) — all line refs against that tag.
- Oracle QEMU pin: **`qemu = 1:10.2.1+ds-1ubuntu3.1`** (Ubuntu 26.04 "resolute", `resolute-updates/main`).
- Ubuntu snapshot: **`snapshot.ubuntu.com` @ `20260701T000000Z`** (frozen, GPG-verified).
- Consensus plan: `.omc/plans/cvm-qemu10-oracle-rebuild-consensus-plan.md`.

---

## What changed (the `[A]` work, done in this repo)

| File | Change |
|------|--------|
| `verifier/builder/Dockerfile` | acpi-builder stage: `debian:bookworm` + `git clone kvinwang/qemu-tdx` → **`ubuntu:26.04` + `apt-get source qemu=1:10.2.1+ds-1ubuntu3.1` + offline-dump patch**. Final stage base → `ubuntu:26.04` (oracle glibc/ABI). ROMs from `qemu-system-data`+`ipxe-qemu` (host-identical). |
| `verifier/builder/patches/0001-dump-acpi-tables.patch` | **New.** Offline ACPI-dump patch, scoped from `kvinwang@dbcec07c` to the DUMP mechanism only (compat machinery dropped → `QEMU_ACPI_COMPAT_VER` inert). Applies fuzz=0 to `1:10.2.1+ds-1ubuntu3.1`. |
| `build/shared/config-qemu.sh` | `SOURCE_DATE_EPOCH`: `git log` → **auto-detect** `dpkg-parsechangelog` (apt-source tree) with git fallback (kms twin). |
| `build/shared/pin-packages.sh` | Auto-detect distro: **Ubuntu → `snapshot.ubuntu.com` + `deb-src`** (N1); Debian path unchanged (kms/gateway twins). |
| `verifier/builder/shared/{qemu,}-pinned-packages.txt` | Blanked for Ubuntu bootstrap-regen (rebuilt by `build-image.sh`). |
| `dstack-mr/src/machine.rs` | Added `versioned_options` bucket regression tests (10.2.1 → single-pass/pic-off). No behaviour change. |
| `dstack-mr/src/acpi.rs` | **VERDICT-GATED — see Phase-0 result below.** |

### Patch scoping (why it is DUMP-only)
The kvinwang commit is 15 files / 711 lines: an offline-dump half (`#ifdef DUMP_ACPI_TABLES`)
and a `QEMU_ACPI_COMPAT_VER` version-dispatch half (`acpi_dump_compat_9_1()`). Applying
the raw patch to 10.2.1 source: the DUMP half applies clean; the compat half's
`aml_pci_pdsm`/`build_link_dev`/…/madt rewrites (acpi-build.c hunks #6–#10) **reject**
because 10.2.1 changed those functions. But 10.2.1's native code already emits `AML_LEVEL`
and the Active-High/Level (`0xd`) MADT xrupt override — i.e. exactly the compat "else"
branch that `QEMU_ACPI_COMPAT_VER=10.2.1` would select. So the compat half is dropped:
inert, zero emitted-byte change, matches the approved ADR. `physmem.c` guest_memfd was
refactored in 10.2.1 (`assert(kvm_enabled())` → `if(!kvm_enabled()) goto out_free`), so
that hunk was re-ported as an `#ifndef DUMP_ACPI_TABLES` wrap of the whole block.

---

## Phase-0 dump-diff verdict (THE GATE)

> Both oracles built offline; `etc/acpi/tables` dumped for Shape-1 (test-host) and
> Shape-2 (prod measurement: `num_gpus=8, num_nvswitches=4, hugepages=False`).

**VERDICT: (i) — only AML bytes moved. `acpi.rs` needs NO code change.**

| Shape | 9.2.1 baseline | 10.2.1 Canonical |
|-------|----------------|------------------|
| Shape-1 (16c/32G, 1 GPU) | `FACS,DSDT(9538),FACP,APIC(240),MCFG,WAET,RSDT` | `FACS,DSDT(9605),FACP,APIC(240),MCFG,WAET,RSDT` |
| Shape-2 (128c/512G, 8 GPU + 4 NVSwitch, hugepages=False) | `FACS,DSDT(19502),FACP,APIC(1136),MCFG,WAET,RSDT` | `FACS,DSDT(19569),FACP,APIC(1136),MCFG,WAET,RSDT` |

Same 7-table set / order / count. Per-table byte diff (both shapes): **FACS, FACP,
APIC, MCFG, WAET are byte-identical**; the whole version delta is **DSDT +67 B of
AML** (constant across shapes → version-intrinsic, not topology) plus RSDT's 4-5
relocated table pointers (mechanical, from the DSDT growth). `build_tables`
(`acpi.rs:197-327`) re-derives offsets/checksums from the dumped blob, so the
growth is auto-absorbed — the new 10.2.1 binary **alone** reconstructs RTMR0.
**D1 stays option (A); the dynamic-loader (C) is not needed.** APIC scales
240→1136 with the PCI topology, byte-identical across versions.

Phase C is therefore a no-op on `acpi.rs`: `machine.rs:64-73` already buckets
`10.2.1 → pic=false/two_pass=false` (regression test added), and
`QEMU_ACPI_COMPAT_VER` is inert (the patch dropped the compat machinery).

RTMR0 **will** move from the 9.2.1 baseline (DSDT AML changed) — the expected,
binary-handled re-baseline. MRTD is firmware-bound and must NOT move (`[H]` Phase F).

**One new 10.2.1 runtime gate** was found and bypassed in the patch:
`target/i386/kvm/tdx.c` rejects a named `-cpu` model for TDX. The oracle uses
`-cpu qemu64` (CPU-model-independent placeholder), so the check is
`#ifndef DUMP_ACPI_TABLES`-guarded. The KVM-path TDX gates (`tdx_finalize_vm`,
phys-bits, CPUID) are unreachable under the TCG dump (registered only in
`tdx_kvm_init`, which `kvm_arch_init` never calls without KVM).

**Fidelity caveat (do not skip):** the offline Shape-2 feeds the *same self-sourced argv*
to both oracles, so verdict (i) does **not** clear the real 8×H200 arg-builder — a
virtee-`#53`-style `-numa` omission cannot be detected offline. That is the GPU-CC E2E
Non-Goal, gated separately. The diff is the **net kvinwang-9.2.1 → Canonical-10.2.1**
delta (version **+** fork/stock together), not an isolated version delta.

---

## `[H]` Host runbook — Phase E (real TD + quote) + Phase F (verify + byte-equality)

Requires a **dedicated, non-prod** Intel TDX host (prod CVM node `146.88.194.8` is NOT
touched). Zero prod impact.

### N3 HARD GATE — assert before trusting any byte-equality
```
oracle QEMU_SRC_VERSION  ==  host apt QEMU version  ==  same pinned Ubuntu snapshot
```
A security bump (`+ds-1ubuntu3.1 → 3.2`) between oracle build and host install silently
voids the proof. Re-pin `ARG QEMU_SRC_VERSION` + `UBUNTU_SNAPSHOT_DATE`, rebuild the
oracle, and re-run everything if the host version moved. Also pin `ipxe-qemu` /
`qemu-system-data` to the host's versions (the option ROMs must match).

### Phase E — install matching QEMU, boot a real TD, capture a quote
```bash
# 1. Install the byte-identical Canonical QEMU on the test host and HOLD it.
sudo apt-get install -y qemu-system-x86=1:10.2.1+ds-1ubuntu3.1 \
                        qemu-system-data=1:10.2.1+ds-1ubuntu3.1 ipxe-qemu
sudo apt-mark hold qemu-system-x86 qemu-system-data qemu-system-common ipxe-qemu
qemu-system-x86_64 --version   # confirm 10.2.1

# 2. Point dstack at the apt QEMU.
#    /etc/dstack/client.conf  ->  [qemu] path = /usr/bin/qemu-system-x86_64

# 3. Boot dstack-nvidia-0.5.11 (image a6eafc5f…) as a TD and capture the quote.
#    Ops gotchas (memory cvm/20260706 §6): slow TDX teardown; `dstack.py run` holds the
#    VFIO fd; use `pgrep qemu` (not -x); `qemu-img -U`. No data disk to orphan.
#    Capture: TDX quote, CCEL (event log), and the fw_cfg etc/table-loader.
```

### OvmfVariant — resolve EMPIRICALLY (do not force)
`verification.rs:260-262` prefers the image's declared `ovmf_variant` stamp; it falls back
to parsing the version from the image name. Resolve by:
1. reading the image `metadata.json` `ovmf_variant` (authoritative), **and**
2. counting real-quote RTMR0 events from the CCEL.

Expected **Pre202505 / 13 events** per memory `20260706 §36` (the `43cf42b0` build stamps
`ovmf_variant` from metadata.json). The plan is correct under **13 or 17** events — the ACPI
delta is 3 events either way. Only if the empirical result is Stable/17 do you also confirm
`bootorder_fwcfg` (`tdvf.rs:361`; `/rom@genroms/linuxboot_dma.bin`) — absent under Pre/13.

### Phase F — verify + byte-equality report
```bash
# Build the local verifier with the rebuilt oracle (also runnable off-host):
GIT_REV=245201be13be06f84b54c416c52b350612aa695e \
  ./verifier/builder/build-image.sh fortunelucky777/dstack-verifier:qemu10-spike

# POST the real quote -> expect: is_valid=true, os_image_hash_verified=true, reason=None
# Report with RUST_LOG=debug + `dstack-mr` CLI + an RTMR0/MRTD extractor. Assert:
#   reconstructed RTMR0 == quote RTMR0
#   reconstructed MRTD  == quote MRTD           (verification.rs:856-868)
# Confirming tests:
#   MRTD(10.2.1) == MRTD(9.2.1 baseline)        (firmware-bound; MUST NOT move)
#   RTMR0 delta  == exactly the 3 ACPI events (acpi_loader / rsdp / tables_hash)
#                   under the resolved OvmfVariant fold; all other events unchanged.
```

### Pre-mortem quick-reference
- **MRTD moved** → same OS image = identical firmware; `≥9.0` → single-pass. Check the
  confirming test fires first.
- **`is_valid=false` with byte-correct ACPI** → inspect `bootorder_fwcfg` /
  `variable_authority` / event count (Stable/17 only); confirm OvmfVariant matches the stamp.
  Under the expected Pre/13 the log has zero non-ACPI QEMU-sensitive events.

---

## Portability — re-homing to a standalone `Dstack-TEE/dstack` checkout (proven 2026-07-10, parked)

This working tree (the `private-ml-sdk/meta-dstack-nvidia/dstack` submodule) **is**
`Dstack-TEE/dstack` (`origin` = `https://github.com/Dstack-TEE/dstack`, tag
`verifier-v0.5.11`), so this entire change-set is already official-repo code. A
standalone port was executed and every file `cmp`-verified byte-identical, then
**parked by decision: keep `private-ml-sdk` as the pinned working base; re-home
later whenever needed.** The proven recipe:

```bash
git clone https://github.com/Dstack-TEE/dstack.git && cd dstack
git checkout 245201be && git switch -c qemu10-oracle-rebuild
git -C <this-tree> diff | git apply                                   # 6 tracked files
cp -r <this-tree>/verifier/builder/QEMU10_ORACLE.md \
      <this-tree>/verifier/builder/patches verifier/builder/          # 2 untracked
```

`build-image.sh` + build contexts resolve identically in-repo, and the oracle build is
reproducible (identical binary sha256 across independent builds), so the standalone
tree yields a byte-exact oracle — the re-home is mechanical whenever it's wanted.

## Follow-ups (not this spike)
- `kms/dstack-app/builder/Dockerfile:46` — identical oracle swap (the twin; still clones kvinwang).
- Upstream ask to `Dstack-TEE/dstack` (or converge with `virtee/tdx-measure`) so the fork leaves the ecosystem.
- Track-2 **destructive** fleet re-baseline, gated on the 8×H200 + NVSwitch GPU-CC E2E.
- `dstack.py:173` hardcoded `hugepages:False` vs the manifest-driven launch path (`:611`).
- **`dstack.py:691` machine-type drift-hardening** (deliberately deferred out of this
  spike — optional, and `dstack.py` is the prod-SHARED launcher). To do it safely:
  derive `pc-q35-{major}.{minor}` from the already-detected `get_qemu_version()`
  (`:1187`) with a **fallback to bare `q35`** if detection fails — a bare `pc-q35-10.2`
  would break the 9.2.1 prod fleet (no such machine). It is measurement-neutral (bare
  `q35` already resolves to `pc-q35-{ver}` on each binary), so it changes no RTMR0.
  Under verdict (i) do NOT co-pin the oracle (`acpi.rs:105` stays bare `q35`). Stamping
  `machine_type` into the measured `vm_config` (`gen_vm_config` `:144`) first requires
  confirming the verifier's `VmConfig` struct tolerates the new field (serde) — validate
  on the test host before shipping.
