// Copyright (c) 2026 Zainiumdynamics. All rights reserved.
// Designed for Zainium OS by Zainiumdynamics — https://zainiumdynamics.tech
//
// Bootloader is Limine (https://github.com/limine-bootloader/limine).
//
// Live/install menus are NOT authored by this installer.
//
// LIVE medium:
//   You prepare and ship:  /overlayer/zaisys/limine/limine.conf
//   (Try / Install / anything — your content. Installer never rewrites live menu.)
//
// AFTER install:
//   You prepare a separate file for the installer to deploy, e.g.:
//     /overlayer/zaisys/limine/limine.install.conf
//   Installer only: substitute ${ROOT_UUID} / ${ROOT_FSTYPE} (if present)
//   then copy → ESP limine.conf and target overlayer/zaisys/limine/limine.conf.
//
// No Try/Install strings hardcoding. No dummy UUIDs.

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use crate::model::{InstallPlan, ProgressState};

use super::{
    assets::{discover, AssetReport},
    config::InstallerConfig,
    paths::{target_root, zaisys},
};

/// Placeholders the installer may fill in *your* install conf (only if they appear).
pub const PH_ROOT_UUID: &str = "${ROOT_UUID}";
pub const PH_ROOT_FSTYPE: &str = "${ROOT_FSTYPE}";

/// Apply only known placeholders. Leaves the rest of the file untouched.
pub fn apply_install_placeholders(
    template: &str,
    root_uuid: &str,
    root_fstype: &str,
) -> Result<String, String> {
    let uuid = root_uuid.trim();
    if uuid.is_empty() {
        return Err("root UUID is empty".into());
    }
    if is_dummy_uuid(uuid) {
        return Err("refusing dummy root UUID".into());
    }
    if template.contains(PH_ROOT_UUID) && uuid.is_empty() {
        return Err(format!("template needs {PH_ROOT_UUID} but UUID missing"));
    }
    let mut out = template.to_string();
    if out.contains(PH_ROOT_UUID) {
        out = out.replace(PH_ROOT_UUID, uuid);
    }
    if out.contains(PH_ROOT_FSTYPE) {
        out = out.replace(PH_ROOT_FSTYPE, root_fstype);
    }
    // Fail if leftover required-looking placeholders remain
    if out.contains(PH_ROOT_UUID) {
        return Err(format!("unresolved {PH_ROOT_UUID} after substitute"));
    }
    Ok(out)
}

fn is_dummy_uuid(uuid: &str) -> bool {
    uuid.contains("00000000")
        || uuid == "REPLACE-WITH-ROOT-UUID"
        || uuid == "PENDING"
        || uuid.eq_ignore_ascii_case("dummy")
}

/// Resolve partition UUID via blkid (no hardcoded values).
pub fn blkid_uuid(device: &str) -> Result<String, String> {
    let out = Command::new("blkid")
        .args(["-s", "UUID", "-o", "value", device])
        .output()
        .map_err(|e| format!("blkid failed to run: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "blkid failed for {device}: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    let uuid = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if uuid.is_empty() {
        return Err(format!("blkid returned empty UUID for {device}"));
    }
    Ok(uuid)
}

/// Find live conf you prepared (not generated here).
pub fn find_live_limine_conf(assets: &AssetReport, cfg: &InstallerConfig) -> Option<PathBuf> {
    if let Some(p) = &cfg.limine_live_conf {
        if p.is_file() {
            return Some(p.clone());
        }
    }
    let mut candidates = Vec::new();
    if let Some(z) = &assets.zaisys {
        candidates.push(z.join("limine/limine.conf"));
        candidates.push(z.join("limine.conf"));
    }
    if let Some(o) = &assets.overlayer {
        candidates.push(o.join("zaisys/limine/limine.conf"));
        candidates.push(o.join("zaisys/limine.conf"));
    }
    candidates.into_iter().find(|p| p.is_file())
}

/// Find install-time conf you prepared for the installer to deploy.
pub fn find_install_limine_conf(assets: &AssetReport, cfg: &InstallerConfig) -> Option<PathBuf> {
    if let Some(p) = &cfg.limine_install_conf {
        if p.is_file() {
            return Some(p.clone());
        }
    }
    let mut candidates = Vec::new();
    if let Some(z) = &assets.zaisys {
        candidates.extend([
            z.join("limine/limine.install.conf"),
            z.join("limine.install.conf"),
            z.join("limine/limine-installed.conf"),
            z.join("limine-installed.conf"),
        ]);
    }
    if let Some(o) = &assets.overlayer {
        candidates.extend([
            o.join("zaisys/limine/limine.install.conf"),
            o.join("zaisys/limine.install.conf"),
            o.join("zaisys/limine/limine-installed.conf"),
        ]);
    }
    candidates.into_iter().find(|p| p.is_file())
}

pub fn job_install_boot(plan: &InstallPlan, progress: &mut ProgressState) -> Result<String, String> {
    let cfg = InstallerConfig::from_env();
    let assets = discover(&cfg);
    let root = target_root(plan);
    let efi = root.join("efi");
    fs::create_dir_all(efi.join("EFI/BOOT")).map_err(|e| e.to_string())?;
    fs::create_dir_all(efi.join("overlayer/zaisys/kernel")).map_err(|e| e.to_string())?;
    let target_zaisys_limine = zaisys(&root).join("limine");
    fs::create_dir_all(&target_zaisys_limine).map_err(|e| e.to_string())?;
    fs::create_dir_all(zaisys(&root).join("kernel")).map_err(|e| e.to_string())?;

    // Report live conf if present (installer does not rewrite it on live medium)
    if let Some(live) = find_live_limine_conf(&assets, &cfg) {
        progress.append_log(format!(
            "limine: live conf present (unchanged by installer): {}",
            live.display()
        ));
        // On target tree after deploy, keep a copy under overlayer/zaisys/limine for reference only
        let dest = target_zaisys_limine.join("limine.conf");
        if live != dest {
            if let Err(e) = fs::copy(&live, &dest) {
                progress.append_log(format!("limine: note — could not stage live conf: {e}"));
            } else {
                progress.append_log(format!("limine: staged live conf → {}", dest.display()));
            }
        }
    } else {
        progress.append_log(
            "limine: no live limine.conf under overlayer/zaisys/limine yet (you will add it before ISO build)",
        );
    }

    // Install conf: use YOUR template if you prepared one; otherwise generate
    // a real working config from discovered assets (kernel/initrd/wallpaper
    // paths we're actually staging, resolved root UUID/fstype) — never a
    // hardcoded fallback string, and never a path we didn't actually stage.
    let install_src = find_install_limine_conf(&assets, &cfg);

    let root_uuid = match resolve_root_uuid(plan, progress) {
        Ok(u) => u,
        // Dry-run smoke test with nothing to substitute into and no real
        // partition yet — still exercise the rest of the pipeline (paths,
        // wallpaper, kernel/initrd staging) with a clearly-marked placeholder
        // rather than failing the whole dry-run over a missing UUID.
        Err(e) if plan.dry_run && install_src.is_none() => {
            progress.append_log(format!(
                "limine: {e} — generating preview conf with placeholder UUID (dry-run only)"
            ));
            "00000000-0000-0000-0000-000000000000".to_string()
        }
        Err(e) => return Err(e),
    };

    let body = match &install_src {
        Some(p) => {
            progress.append_log(format!("limine: install conf source {}", p.display()));
            let template = fs::read_to_string(p).map_err(|e| e.to_string())?;
            apply_install_placeholders(&template, &root_uuid, &cfg.root_fstype)?
        }
        None => {
            progress.append_log(
                "limine: no limine.install.conf found — generating one from discovered assets",
            );
            generate_install_conf(&assets, &cfg, &root_uuid)
        }
    };

    // Limine only ever looks for a file actually named `limine.conf` (or
    // legacy `limine.cfg`) — checked first at `<EFI app path>/limine.conf`,
    // i.e. EFI/BOOT/limine.conf since that's where BOOTX64.EFI is installed
    // (see install_limine_efi below).
    let esp_conf = efi.join("EFI/BOOT/limine.conf");
    fs::create_dir_all(esp_conf.parent().unwrap()).map_err(|e| e.to_string())?;
    fs::write(&esp_conf, &body).map_err(|e| e.to_string())?;
    // Also write as active conf on target overlayer/zaisys/limine (replaces live menu after install)
    // — this copy is our own on-disk staging convention for reference/audit,
    // not read by Limine itself at boot time.
    fs::write(target_zaisys_limine.join("limine.conf"), &body).map_err(|e| e.to_string())?;
    // Keep original template on target for audits (or the generated conf itself, if we authored it)
    match &install_src {
        Some(src) => {
            fs::copy(src, target_zaisys_limine.join("limine.install.conf"))
                .map_err(|e| e.to_string())?;
        }
        None => {
            fs::write(target_zaisys_limine.join("limine.install.conf"), &body)
                .map_err(|e| e.to_string())?;
        }
    }

    progress.append_log(format!(
        "limine: wrote installed conf → {} (uuid={root_uuid})",
        esp_conf.display()
    ));

    copy_boot_payloads(&assets, &efi, progress)?;

    if !plan.dry_run {
        let missing = assets.missing_for_real_install();
        if !missing.is_empty() {
            return Err(format!(
                "cannot finalize bootloader; missing: {}",
                missing.join(", ")
            ));
        }
        install_limine_efi(&assets, &efi, progress)?;
    } else {
        progress.append_log("limine: dry-run — skipped EFI binary install (copy when assets exist)");
    }

    Ok(format!("limine conf → {}", esp_conf.display()))
}

fn resolve_root_uuid(plan: &InstallPlan, progress: &mut ProgressState) -> Result<String, String> {
    // Prefer UUID produced by format job
    if let Some(u) = plan.root_uuid.as_ref().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()) {
        if !is_dummy_uuid(&u) {
            progress.append_log("limine: root UUID from install plan (post-format)");
            return Ok(u);
        }
    }
    if let Ok(u) = std::env::var("ZAINIUM_ROOT_UUID") {
        let u = u.trim().to_string();
        if !u.is_empty() {
            progress.append_log("limine: root UUID from ZAINIUM_ROOT_UUID");
            return Ok(u);
        }
    }
    if let Some(part) = plan.root_part.as_ref() {
        if part != "dry-run-root" {
            if let Ok(u) = blkid_uuid(part) {
                progress.append_log(format!("limine: root UUID from blkid {part}"));
                return Ok(u);
            }
        }
    }
    if let Some(disk) = &plan.disk {
        if let Ok(u) = blkid_uuid(&disk.path) {
            progress.append_log(format!("limine: root UUID from blkid {}", disk.path));
            return Ok(u);
        }
        for suffix in ["2", "p2"] {
            let part = format!("{}{suffix}", disk.path);
            if Path::new(&part).exists() {
                if let Ok(u) = blkid_uuid(&part) {
                    progress.append_log(format!("limine: root UUID from blkid {part}"));
                    return Ok(u);
                }
            }
        }
    }
    if plan.dry_run {
        return Err(
            "dry-run: set ZAINIUM_ROOT_UUID when testing install conf substitution".into(),
        );
    }
    Err("could not determine root UUID (blkid / ZAINIUM_ROOT_UUID)".into())
}

/// Real fixed convention — no `/boot`, no configurable ESP scheme. The
/// kernel's own filename is preserved as-is (it isn't fixed the way
/// quantra-ramfs.img is — your kernel build can be named anything, e.g.
/// "zainix"); quantra-ramfs.img and wallpaper.bmp are fixed, real names.
const ESP_KERNEL_DIR: &str = "overlayer/zaisys/kernel";
const ESP_INITRD_NAME: &str = "quantra-ramfs.img";
const ESP_WALLPAPER_REL: &str = "overlayer/zaisys/limine/wallpaper.bmp";

fn copy_boot_payloads(
    assets: &AssetReport,
    efi: &Path,
    progress: &mut ProgressState,
) -> Result<(), String> {
    let kernel_dir = efi.join(ESP_KERNEL_DIR);
    fs::create_dir_all(&kernel_dir).map_err(|e| e.to_string())?;
    if let Some(src) = &assets.kernel {
        let name = src
            .file_name()
            .ok_or_else(|| format!("kernel path has no filename: {}", src.display()))?;
        let kdest = kernel_dir.join(name);
        fs::copy(src, &kdest).map_err(|e| e.to_string())?;
        progress.append_log(format!(
            "limine: copied kernel {} → {}",
            src.display(),
            kdest.display()
        ));
    }
    if let Some(src) = &assets.initrd {
        let idest = kernel_dir.join(ESP_INITRD_NAME);
        fs::copy(src, &idest).map_err(|e| e.to_string())?;
        progress.append_log(format!(
            "limine: copied quantra-ramfs {} → {}",
            src.display(),
            idest.display()
        ));
    }
    if let Some(src) = &assets.wallpaper {
        let wdest = efi.join(ESP_WALLPAPER_REL);
        if let Some(parent) = wdest.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        fs::copy(src, &wdest).map_err(|e| e.to_string())?;
        progress.append_log(format!(
            "limine: copied wallpaper {} → {}",
            src.display(),
            wdest.display()
        ));
    }
    Ok(())
}

/// Build a working limine.conf from real discovered assets (kernel/initrd/
/// wallpaper paths we're actually staging, resolved root UUID/fstype). Used
/// only when no user-authored install template exists — never a fixed
/// string, and never a path we didn't actually stage. The kernel's real
/// filename (not an assumed "vmlinuz") is used in `path:`.
fn generate_install_conf(assets: &AssetReport, cfg: &InstallerConfig, root_uuid: &str) -> String {
    let mut globals = String::from("timeout: 5\ndefault_entry: 1\ngraphics: yes\n");
    if assets.wallpaper.is_some() {
        globals.push_str(&format!("wallpaper: boot():/{ESP_WALLPAPER_REL}\n"));
    }
    // NOTE: real Limine has no single `theme: dark/light` toggle — only
    // individual `theme_*`-prefixed options (theme_background, etc). `cfg.theme`
    // is just our own dark/light convenience setting; there's no valid Limine
    // option to translate it into yet, so it's intentionally not emitted
    // rather than writing something Limine would reject.
    let kernel_name = assets
        .kernel
        .as_ref()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "MISSING-KERNEL".to_string());
    format!(
        "# Limine installed boot — generated by carve\n\
         # (no limine.install.conf found under overlayer/zaisys/limine; built from discovered assets)\n\
         {globals}\n\
         /Zainium\n\
         \x20\x20\x20\x20protocol: linux\n\
         \x20\x20\x20\x20path: boot():/{ESP_KERNEL_DIR}/{kernel_name}\n\
         \x20\x20\x20\x20cmdline: quiet root=UUID={root_uuid} rootfstype={}\n\
         \x20\x20\x20\x20module_path: boot():/{ESP_KERNEL_DIR}/{ESP_INITRD_NAME}\n",
        cfg.root_fstype,
    )
}

fn install_limine_efi(
    assets: &AssetReport,
    efi: &Path,
    progress: &mut ProgressState,
) -> Result<(), String> {
    let efi_src = assets
        .limine_efi
        .as_ref()
        .ok_or("Limine EFI binary not found under overlayer/zaisys/limine")?;
    let dest = efi.join("EFI/BOOT/BOOTX64.EFI");
    fs::create_dir_all(dest.parent().unwrap()).map_err(|e| e.to_string())?;
    fs::copy(efi_src, &dest).map_err(|e| e.to_string())?;
    progress.append_log(format!(
        "limine: installed EFI {} → {}",
        efi_src.display(),
        dest.display()
    ));
    Ok(())
}

/// CLI: process install conf template → out (your file + UUID only).
pub fn apply_install_conf_file(
    src: &Path,
    out: &Path,
    root_uuid: &str,
    root_fstype: &str,
) -> Result<(), String> {
    let template = fs::read_to_string(src).map_err(|e| e.to_string())?;
    let body = apply_install_placeholders(&template, root_uuid, root_fstype)?;
    if let Some(parent) = out.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::write(out, body).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn substitute_only_placeholders_no_menu_invention() {
        let t = "timeout: 5\n/My Entry\n    cmdline: root=UUID=${ROOT_UUID} rootfstype=${ROOT_FSTYPE}\n";
        let out = apply_install_placeholders(t, "abcdef12-3456-7890-abcd-ef1234567890", "ext4")
            .unwrap();
        assert!(out.contains("root=UUID=abcdef12-3456-7890-abcd-ef1234567890"));
        assert!(out.contains("rootfstype=ext4"));
        assert!(out.contains("/My Entry"));
        assert!(!out.contains("Try Zainium"));
        assert!(!out.contains("${ROOT_UUID}"));
    }

    #[test]
    fn rejects_dummy_uuid() {
        let t = "root=UUID=${ROOT_UUID}\n";
        assert!(apply_install_placeholders(t, "PENDING", "ext4").is_err());
        assert!(apply_install_placeholders(t, "00000000-0000-0000-0000-000000000000", "ext4")
            .is_err());
    }

    #[test]
    fn leaves_file_without_placeholders_intact() {
        let t = "# my hand-written conf\ntimeout: 3\n";
        let out = apply_install_placeholders(t, "abcdef12-3456-7890-abcd-ef1234567890", "xfs")
            .unwrap();
        assert_eq!(out, t);
    }
}
