// Copyright (c) 2026 Zainiumdynamics. All rights reserved.
// Designed for Zainium OS by Zainiumdynamics — https://zainiumdynamics.tech
//
// Discover boot/OS assets at runtime. Kernel, Limine, and the packed root
// image may not exist yet — that is reported honestly; we never invent fake
// binaries.

use std::{
    fs,
    path::{Path, PathBuf},
};

use super::config::InstallerConfig;

#[derive(Clone, Debug, Default)]
pub struct AssetReport {
    pub overlayer: Option<PathBuf>,
    pub syshub: Option<PathBuf>,
    pub zaisys: Option<PathBuf>,
    pub kernel: Option<PathBuf>,
    pub initrd: Option<PathBuf>,
    /// The Limine EFI binary (BOOTX64.EFI) staged for the live/install media.
    pub limine_efi: Option<PathBuf>,
    /// Your prepared LIVE menu: overlayer/zaisys/limine/limine.conf
    pub limine_live_conf: Option<PathBuf>,
    /// Your prepared INSTALL-time conf for installer to deploy: limine.install.conf
    pub limine_install_conf: Option<PathBuf>,
    /// Boot menu wallpaper: overlayer/zaisys/limine/wallpaper.bmp
    pub wallpaper: Option<PathBuf>,
    pub squash: Option<PathBuf>,
    pub quantra: Option<PathBuf>,
    pub notes: Vec<String>,
}

impl AssetReport {
    pub fn has_os_source(&self) -> bool {
        self.squash.is_some()
            || self
                .syshub
                .as_ref()
                .is_some_and(|p| p.is_dir())
    }

    /// Format of the packed root image at `self.squash`, inferred from its
    /// extension: `.img` (eclipse-iso-builder's EROFS default) vs
    /// `.squash`/`.squashfs`. The field is still named `squash` for
    /// backward compat (ZAINIUM_SQUASH, InstallMode::ExpandSquash) — it
    /// means "the packed root image," not "specifically a SquashFS file."
    pub fn squash_format(&self) -> &'static str {
        image_format(self.squash.as_deref())
    }

    pub fn has_boot_payload(&self) -> bool {
        self.kernel.is_some() && self.initrd.is_some()
    }

    pub fn has_limine_efi(&self) -> bool {
        self.limine_efi.is_some()
    }

    /// Real disk install requirements. `limine_install_conf` is NOT listed
    /// here — the installer generates a working config itself from these
    /// same discovered assets when you haven't prepared your own template
    /// (see `limine::generate_install_conf`).
    pub fn missing_for_real_install(&self) -> Vec<&'static str> {
        let mut m = Vec::new();
        if !self.has_os_source() {
            m.push("OS source (zairoot.img/zairoot.squash or live overlayer/syshub)");
        }
        if self.kernel.is_none() {
            m.push("kernel (any file under overlayer/zaisys/kernel/ besides quantra-ramfs.img, or ZAINIUM_KERNEL)");
        }
        if self.initrd.is_none() {
            m.push("initramfs (overlayer/zaisys/kernel/quantra-ramfs.img or ZAINIUM_INITRD)");
        }
        if self.limine_efi.is_none() {
            m.push("Limine EFI (overlayer/zaisys/limine/BOOTX64.EFI)");
        }
        m
    }

    pub fn to_toml_report(&self) -> String {
        let mut s = String::from("# carve asset discovery\n# No assets are invented.\n\n");
        fn line(s: &mut String, k: &str, v: &Option<PathBuf>) {
            match v {
                Some(p) => s.push_str(&format!("{k} = \"{}\"\n", p.display())),
                None => s.push_str(&format!("{k} = \"\"\n")),
            }
        }
        line(&mut s, "overlayer", &self.overlayer);
        line(&mut s, "syshub", &self.syshub);
        line(&mut s, "zaisys", &self.zaisys);
        line(&mut s, "kernel", &self.kernel);
        line(&mut s, "initrd", &self.initrd);
        line(&mut s, "limine_efi", &self.limine_efi);
        line(&mut s, "limine_live_conf", &self.limine_live_conf);
        line(&mut s, "limine_install_conf", &self.limine_install_conf);
        line(&mut s, "wallpaper", &self.wallpaper);
        line(&mut s, "squash", &self.squash);
        if self.squash.is_some() {
            s.push_str(&format!("squash_format = \"{}\"\n", self.squash_format()));
        }
        line(&mut s, "quantra", &self.quantra);
        s.push_str(&format!("has_os_source = {}\n", self.has_os_source()));
        s.push_str(&format!("has_boot_payload = {}\n", self.has_boot_payload()));
        s.push_str(&format!("has_limine_efi = {}\n", self.has_limine_efi()));
        if !self.notes.is_empty() {
            s.push_str("\n# notes\n");
            for n in &self.notes {
                s.push_str(&format!("# - {n}\n"));
            }
        }
        s
    }
}

/// Discover assets from config + optional search roots.
pub fn discover(cfg: &InstallerConfig) -> AssetReport {
    let mut report = AssetReport::default();

    // Explicit env overrides first
    if let Ok(p) = std::env::var("ZAINIUM_KERNEL") {
        let p = PathBuf::from(p);
        if p.is_file() {
            report.kernel = Some(p);
        }
    }
    if let Ok(p) = std::env::var("ZAINIUM_INITRD") {
        let p = PathBuf::from(p);
        if p.is_file() {
            report.initrd = Some(p);
        }
    }
    if let Some(p) = &cfg.limine_live_conf {
        if p.is_file() {
            report.limine_live_conf = Some(p.clone());
        }
    }
    if let Some(p) = &cfg.limine_install_conf {
        if p.is_file() {
            report.limine_install_conf = Some(p.clone());
        }
    }

    let mut roots: Vec<PathBuf> = Vec::new();
    if let Some(z) = &cfg.zairoot {
        roots.push(z.clone());
    }
    if let Some(o) = &cfg.overlayer {
        roots.push(o.parent().unwrap_or(o.as_path()).to_path_buf());
    }

    // Relative / conventional live locations (no host username baked in)
    for c in [
        "/run/live",
        "/live",
        "/mnt/live",
        "/zairoot",
        ".",
    ] {
        roots.push(PathBuf::from(c));
    }

    // Resolve overlayer / zaisys / squash from config
    if let Some(o) = &cfg.overlayer {
        if o.is_dir() {
            report.overlayer = Some(o.clone());
        }
    }
    if let Some(z) = &cfg.zaisys {
        if z.is_dir() {
            report.zaisys = Some(z.clone());
        }
    }
    if let Some(s) = &cfg.squash {
        if s.is_file() {
            report.squash = Some(s.clone());
        }
    }

    for root in &roots {
        probe_root(root, &mut report);
    }

    if report.kernel.is_none() {
        report.notes.push(
            "kernel not found — add under overlayer/zaisys/kernel/ or set ZAINIUM_KERNEL".into(),
        );
    }
    if report.initrd.is_none() {
        report.notes.push(
            "quantra-ramfs.img not found — build it under overlayer/zaisys/kernel/ or set ZAINIUM_INITRD"
                .into(),
        );
    }
    if report.limine_efi.is_none() {
        report
            .notes
            .push("Limine EFI not found — add BOOTX64.EFI under overlayer/zaisys/limine/ when ready".into());
    }
    if report.limine_live_conf.is_none() {
        report.notes.push(
            "live limine.conf not found — prepare overlayer/zaisys/limine/limine.conf yourself"
                .into(),
        );
    }
    if report.limine_install_conf.is_none() {
        report.notes.push(
            "install conf not found — prepare overlayer/zaisys/limine/limine.install.conf for installer to copy"
                .into(),
        );
    }
    if report.squash.is_none() {
        report.notes.push(
            "zairoot.img/zairoot.squash not found — optional until image factory packs one"
                .into(),
        );
    }
    if !report.has_os_source() {
        report.notes.push(
            "no OS source tree yet — set ZAINIUM_OVERLAYER or ZAINIUM_SQUASH for deploy".into(),
        );
    }

    report
}

fn probe_root(root: &Path, report: &mut AssetReport) {
    if !root.exists() {
        return;
    }

    let overlayer_candidates = [
        root.join("overlayer"),
        root.to_path_buf(), // root may already be overlayer
    ];

    for ol in &overlayer_candidates {
        let syshub = ol.join("syshub");
        let zaisys = ol.join("zaisys");
        if syshub.is_dir() {
            report.overlayer.get_or_insert_with(|| ol.clone());
            report.syshub.get_or_insert(syshub.clone());
            let q = syshub.join("engine/quantra");
            if q.is_file() {
                report.quantra.get_or_insert(q);
            }
        }
        if zaisys.is_dir() {
            report.zaisys.get_or_insert(zaisys.clone());
            scan_zaisys(&zaisys, report);
        }
    }

    // Packed root image next to root / under zaisys — .img (EROFS, eclipse-iso-builder
    // default) checked alongside the legacy .squash/.squashfs names.
    for name in [
        "zairoot.img",
        "zainium.img",
        "zairoot.squash",
        "zairoot.squashfs",
        "zainium.squash",
    ] {
        let p = root.join(name);
        if p.is_file() {
            report.squash.get_or_insert(p);
        }
        if let Some(z) = &report.zaisys {
            let p = z.join(name);
            if p.is_file() {
                report.squash.get_or_insert(p);
            }
        }
    }
}

fn scan_zaisys(zaisys: &Path, report: &mut AssetReport) {
    // overlayer/zaisys/kernel/ holds exactly two files by convention:
    // quantra-ramfs.img (fixed, real name — quantra-ramfs's own build
    // output) and the kernel image, whose name is NOT assumed — no
    // "vmlinuz"/"bzImage" guessing. Whatever else is in this directory is
    // the kernel; ZAINIUM_KERNEL still overrides explicitly if needed.
    let kernel_dir = zaisys.join("kernel");
    if kernel_dir.is_dir() {
        if let Ok(rd) = fs::read_dir(&kernel_dir) {
            for e in rd.flatten() {
                if !e.path().is_file() {
                    continue;
                }
                if e.file_name() == "quantra-ramfs.img" {
                    report.initrd.get_or_insert(e.path());
                } else {
                    report.kernel.get_or_insert(e.path());
                }
            }
        }
    }

    // overlayer/zaisys/limine/ — Limine EFI binary + your conf files.
    let limine_dir = zaisys.join("limine");
    if limine_dir.is_dir() {
        for name in ["BOOTX64.EFI", "bootx64.efi", "limine.efi"] {
            let p = limine_dir.join(name);
            if p.is_file() {
                report.limine_efi.get_or_insert(p);
            }
        }
        if report.limine_efi.is_none() {
            if let Ok(rd) = fs::read_dir(&limine_dir) {
                for e in rd.flatten() {
                    let n = e.file_name().to_string_lossy().to_lowercase();
                    if n.ends_with(".efi") && e.path().is_file() {
                        report.limine_efi.get_or_insert(e.path());
                    }
                }
            }
        }

        // Your conf files (not generated by installer)
        let live = limine_dir.join("limine.conf");
        if live.is_file() {
            report.limine_live_conf.get_or_insert(live);
        }
        for name in ["limine.install.conf", "limine-installed.conf"] {
            let p = limine_dir.join(name);
            if p.is_file() {
                report.limine_install_conf.get_or_insert(p);
            }
        }

        let wallpaper = limine_dir.join("wallpaper.bmp");
        if wallpaper.is_file() {
            report.wallpaper.get_or_insert(wallpaper);
        }
    }

    // Also accept conf files staged directly under zaisys/ (no limine/ subdir yet).
    let live_flat = zaisys.join("limine.conf");
    if live_flat.is_file() {
        report.limine_live_conf.get_or_insert(live_flat);
    }
    for name in ["limine.install.conf", "limine-installed.conf"] {
        let p = zaisys.join(name);
        if p.is_file() {
            report.limine_install_conf.get_or_insert(p);
        }
    }
}

/// Infer packed-root-image format from its extension: `.img` → EROFS
/// (eclipse-iso-builder default), anything else → SquashFS (legacy).
pub fn image_format(path: Option<&Path>) -> &'static str {
    match path.and_then(|p| p.extension()).and_then(|e| e.to_str()) {
        Some("img") => "erofs",
        _ => "squashfs",
    }
}

pub fn write_report(path: &Path, report: &AssetReport) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::write(path, report.to_toml_report()).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_format_detects_erofs_by_extension() {
        assert_eq!(image_format(Some(Path::new("zairoot.img"))), "erofs");
        assert_eq!(image_format(Some(Path::new("/a/b/zainium.img"))), "erofs");
    }

    #[test]
    fn image_format_defaults_to_squashfs() {
        assert_eq!(image_format(Some(Path::new("zairoot.squash"))), "squashfs");
        assert_eq!(image_format(Some(Path::new("zairoot.squashfs"))), "squashfs");
        assert_eq!(image_format(None), "squashfs");
    }

    #[test]
    fn asset_report_squash_format_matches_discovered_path() {
        let mut report = AssetReport::default();
        report.squash = Some(PathBuf::from("zaisys/zairoot.img"));
        assert_eq!(report.squash_format(), "erofs");
    }
}
