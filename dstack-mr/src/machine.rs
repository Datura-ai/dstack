// SPDX-FileCopyrightText: © 2025 Phala Network <dstack@phala.network>
//
// SPDX-License-Identifier: Apache-2.0

use crate::acpi::Tables;
use crate::tdvf::Tdvf;
use crate::util::debug_print_log;
use crate::{kernel, OvmfVariant, RtmrLogs, TdxMeasurements};
use crate::{measure_log, measure_sha384};
use anyhow::{bail, Context, Result};
use fs_err as fs;
use log::debug;

#[derive(Debug, bon::Builder)]
pub struct Machine<'a> {
    pub cpu_count: u32,
    pub memory_size: u64,
    pub firmware: &'a str,
    pub kernel: &'a str,
    pub initrd: &'a str,
    pub kernel_cmdline: &'a str,
    pub two_pass_add_pages: Option<bool>,
    pub pic: Option<bool>,
    pub qemu_version: Option<String>,
    #[builder(default = false)]
    pub smm: bool,
    pub pci_hole64_size: Option<u64>,
    pub hugepages: bool,
    pub num_gpus: u32,
    pub num_nvswitches: u32,
    pub hotplug_off: bool,
    pub root_verity: bool,
    #[builder(default)]
    pub host_share_mode: String,
    /// Selects which OVMF measurement event layout to expect.
    /// Defaults to the pre-edk2-stable202505 layout for backwards compatibility.
    #[builder(default)]
    pub ovmf_variant: OvmfVariant,
}

fn parse_version_tuple(v: &str) -> Result<(u32, u32, u32)> {
    let parts: Vec<u32> = v
        .split('.')
        .map(|p| p.parse::<u32>().context("Invalid version number"))
        .collect::<Result<Vec<_>, _>>()?;
    if parts.len() != 3 {
        bail!(
            "Version string must have exactly 3 parts (major.minor.patch), got {}",
            parts.len()
        );
    }
    Ok((parts[0], parts[1], parts[2]))
}

impl Machine<'_> {
    pub fn versioned_options(&self) -> Result<VersionedOptions> {
        let version = match &self.qemu_version {
            Some(v) => Some(parse_version_tuple(v).context("Failed to parse QEMU version")?),
            None => None,
        };
        let default_pic;
        let default_two_pass;
        let version = version.unwrap_or((9, 1, 0));
        if version < (8, 0, 0) {
            bail!("Unsupported QEMU version: {version:?}");
        }
        if ((8, 0, 0)..(9, 0, 0)).contains(&version) {
            default_pic = true;
            default_two_pass = true;
        } else {
            default_pic = false;
            default_two_pass = false;
        };
        Ok(VersionedOptions {
            version,
            pic: self.pic.unwrap_or(default_pic),
            two_pass_add_pages: self.two_pass_add_pages.unwrap_or(default_two_pass),
            // QEMU >= 10 stopped patching the bzImage setup header (type_of_loader,
            // loadflags, cmdline/initrd addresses) before exposing the kernel via
            // fw_cfg for measured direct boot, so the firmware measures the RAW
            // image. Pre-10 patches the header first, so RTMR1 measures the patched
            // image (see kernel::rtmr1_log).
            patch_kernel_setup_header: version < (10, 0, 0),
        })
    }
}

pub struct VersionedOptions {
    pub version: (u32, u32, u32),
    pub pic: bool,
    pub two_pass_add_pages: bool,
    pub patch_kernel_setup_header: bool,
}

#[cfg(test)]
mod versioned_options_tests {
    use super::*;

    fn machine_with_version(v: Option<&str>) -> Machine<'static> {
        Machine::builder()
            .cpu_count(4)
            .memory_size(4u64 << 30)
            .firmware("/dev/null")
            .kernel("/dev/null")
            .initrd("/dev/null")
            .kernel_cmdline("")
            .hugepages(false)
            .num_gpus(0)
            .num_nvswitches(0)
            .hotplug_off(false)
            .root_verity(false)
            .maybe_qemu_version(v.map(String::from))
            .build()
    }

    /// The Canonical QEMU-10.2.1 oracle relies on the >=9.0 bucket: single-pass
    /// add-pages and pic=off. If a future change re-buckets 10.x this regresses
    /// RTMR0/MRTD reconstruction. `QEMU_ACPI_COMPAT_VER` is inert on the 10.2.1
    /// binary (native tables), so the version only steers pic/two_pass here.
    #[test]
    fn buckets_qemu_10_2_1_as_single_pass_no_pic() {
        let m = machine_with_version(Some("10.2.1"));
        let vo = m.versioned_options().unwrap();
        assert_eq!(vo.version, (10, 2, 1));
        assert!(!vo.pic, "10.2.1 must default to pic=off");
        assert!(!vo.two_pass_add_pages, "10.2.1 must default to two_pass=off");
    }

    #[test]
    fn buckets_9_x_as_single_pass_no_pic() {
        for v in ["9.0.0", "9.1.0", "9.2.1"] {
            let vo = machine_with_version(Some(v)).versioned_options().unwrap();
            assert!(!vo.pic && !vo.two_pass_add_pages, "{v} must be false/false");
        }
    }

    #[test]
    fn buckets_8_x_as_two_pass_pic() {
        for v in ["8.0.0", "8.2.0", "8.5.0"] {
            let vo = machine_with_version(Some(v)).versioned_options().unwrap();
            assert!(vo.pic && vo.two_pass_add_pages, "{v} must be true/true");
        }
    }

    /// RTMR1's kernel measurement depends on the QEMU generation: < 10 measures
    /// the setup-header-patched bzImage, >= 10 measures the raw bzImage. The
    /// Canonical QEMU-10.2.1 quote only reconstructs when 10.x skips the patch.
    /// If a future change re-buckets this, RTMR1 reconstruction regresses.
    #[test]
    fn patches_setup_header_below_qemu_10_only() {
        for v in ["8.0.0", "8.2.0", "9.0.0", "9.1.0", "9.2.1"] {
            let vo = machine_with_version(Some(v)).versioned_options().unwrap();
            assert!(
                vo.patch_kernel_setup_header,
                "{v} must patch the setup header (patched bzImage measured)"
            );
        }
        for v in ["10.0.0", "10.2.1", "11.0.0"] {
            let vo = machine_with_version(Some(v)).versioned_options().unwrap();
            assert!(
                !vo.patch_kernel_setup_header,
                "{v} must skip the patch (raw bzImage measured)"
            );
        }
    }

    #[test]
    fn rejects_pre_8_0() {
        assert!(machine_with_version(Some("7.2.0"))
            .versioned_options()
            .is_err());
    }

    #[test]
    fn none_defaults_to_9_1_0_bucket() {
        let vo = machine_with_version(None).versioned_options().unwrap();
        assert_eq!(vo.version, (9, 1, 0));
        assert!(!vo.pic && !vo.two_pass_add_pages);
        assert!(vo.patch_kernel_setup_header, "9.1.0 default must patch");
    }

    /// Explicit overrides must win over the version default (used by the
    /// per-machine VmConfig knobs).
    #[test]
    fn explicit_overrides_win() {
        let m = Machine::builder()
            .cpu_count(4)
            .memory_size(4u64 << 30)
            .firmware("/dev/null")
            .kernel("/dev/null")
            .initrd("/dev/null")
            .kernel_cmdline("")
            .hugepages(false)
            .num_gpus(0)
            .num_nvswitches(0)
            .hotplug_off(false)
            .root_verity(false)
            .qemu_version("10.2.1".to_string())
            .pic(true)
            .two_pass_add_pages(true)
            .build();
        let vo = m.versioned_options().unwrap();
        assert!(vo.pic && vo.two_pass_add_pages);
    }
}

#[derive(Debug, Clone)]
pub struct TdxMeasurementDetails {
    pub measurements: TdxMeasurements,
    pub rtmr_logs: RtmrLogs,
    pub acpi_tables: Tables,
}

impl Machine<'_> {
    pub fn measure(&self) -> Result<TdxMeasurements> {
        self.measure_with_logs().map(|details| details.measurements)
    }

    pub fn measure_with_logs(&self) -> Result<TdxMeasurementDetails> {
        debug!("measuring machine: {self:#?}");
        let fw_data = fs::read(self.firmware)?;
        let kernel_data = fs::read(self.kernel)?;
        let initrd_data = fs::read(self.initrd)?;
        let tdvf = Tdvf::parse(&fw_data).context("Failed to parse TDVF metadata")?;

        let mrtd = tdvf.mrtd(self).context("Failed to compute MR TD")?;

        let (rtmr0_log, acpi_tables) = tdvf
            .rtmr0_log(self)
            .context("Failed to compute RTMR0 log")?;
        debug_print_log("RTMR0", &rtmr0_log);
        let rtmr0 = measure_log(&rtmr0_log);

        let opts = self.versioned_options()?;
        let rtmr1_log = kernel::rtmr1_log(
            &kernel_data,
            initrd_data.len() as u32,
            self.memory_size,
            0x28000,
            opts.patch_kernel_setup_header,
        )?;
        debug_print_log("RTMR1", &rtmr1_log);
        let rtmr1 = measure_log(&rtmr1_log);

        let rtmr2_log = vec![
            kernel::measure_cmdline(self.kernel_cmdline),
            measure_sha384(&initrd_data),
        ];
        debug_print_log("RTMR2", &rtmr2_log);
        let rtmr2 = measure_log(&rtmr2_log);

        Ok(TdxMeasurementDetails {
            measurements: TdxMeasurements {
                mrtd,
                rtmr0,
                rtmr1,
                rtmr2,
            },
            rtmr_logs: [rtmr0_log, rtmr1_log, rtmr2_log],
            acpi_tables,
        })
    }
}
