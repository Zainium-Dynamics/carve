// Copyright (c) 2026 Zainiumdynamics. All rights reserved.
// Designed for Zainium OS by Zainiumdynamics — https://zainiumdynamics.tech
//
// Deploy OS from real squash or live overlayer only — no fake syshub stubs.

use std::{
    fs,
    path::Path,
    process::Command,
};

use crate::model::{InstallMode, InstallPlan, ProgressState};

use super::{
    assets::{discover, image_format},
    config::InstallerConfig,
    paths::{syshub, target_root, zaisys, zexlib_registry, zexlib_union, zexlib_work},
};

pub fn job_seed_tree(plan: &InstallPlan, progress: &mut ProgressState) -> Result<String, String> {
    let root = target_root(plan);
    let dirs = [
        zaisys(&root).join("kernel"),
        zaisys(&root).join("limine"),
        zexlib_union(&root),
        zexlib_work(&root),
        zexlib_registry(&root),
        root.join("home"),
        root.join("dev"),
        root.join("proc"),
        root.join("sys"),
        root.join("run"),
    ];
    for d in &dirs {
        fs::create_dir_all(d).map_err(|e| format!("mkdir {}: {e}", d.display()))?;
    }
    progress.append_log(format!("seed: layout under {}", root.display()));
    Ok("seed ok".into())
}

pub fn job_deploy_os(plan: &InstallPlan, progress: &mut ProgressState) -> Result<String, String> {
    let cfg = InstallerConfig::from_env();
    let assets = discover(&cfg);
    let root = target_root(plan);

    // Prefer plan paths, then discovered
    let squash = plan
        .squash_path
        .clone()
        .filter(|p| p.is_file())
        .or(assets.squash.clone());
    let overlayer = plan
        .live_overlayer
        .clone()
        .filter(|p| p.is_dir())
        .or(assets.overlayer.clone());

    let result = match plan.mode {
        InstallMode::ExpandSquash => {
            if let Some(sq) = squash {
                deploy_packed_image(&sq, &root, progress)
            } else if let Some(ol) = overlayer {
                progress.append_log(
                    "deploy: packed image missing — using live overlayer rsync (ExpandSquash fallback)",
                );
                deploy_rsync(&ol, &root, plan, progress)
            } else {
                deploy_missing(plan, progress)
            }
        }
        InstallMode::RsyncLiveTree => {
            let ol = overlayer.ok_or_else(|| {
                "no live overlayer found (set ZAINIUM_OVERLAYER)".to_string()
            })?;
            deploy_rsync(&ol, &root, plan, progress)
        }
        InstallMode::KeepSquash => {
            let sq = squash.ok_or_else(|| {
                "no zairoot.img/zairoot.squash found (set ZAINIUM_SQUASH)".to_string()
            })?;
            deploy_keep_squash(&sq, &root, progress)
        }
    };

    if result.is_ok() && !plan.dry_run {
        remove_installer_from_target(&root, progress);
        lock_boot_critical_files(&root, progress);
    }
    result
}

/// `chattr +i` on the boot kernel/initramfs/bootloader files -- a real,
/// kernel-enforced (ext4/btrfs immutable inode flag) block on unlink/write
/// that holds regardless of which tool touches the file, unlike
/// usercore::protect (userutiles-only, opt-in per binary). Best-effort:
/// chattr isn't available/meaningful on every target filesystem (e.g.
/// tmpfs), so a failure here is logged, not a deploy failure.
fn lock_boot_critical_files(root: &Path, progress: &mut ProgressState) {
    let sys = zaisys(root);
    let dirs = [sys.join("kernel"), sys.join("limine")];
    for dir in dirs {
        let entries = match fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            match Command::new("chattr").arg("+i").arg(&path).status() {
                Ok(status) if status.success() => {
                    progress.append_log(format!("deploy: locked {} (chattr +i)", path.display()));
                }
                Ok(status) => progress.append_log(format!(
                    "deploy: WARN — chattr +i on {} exited {status}",
                    path.display()
                )),
                Err(e) => progress.append_log(format!(
                    "deploy: WARN — chattr not available, {} left unlocked: {e}",
                    path.display()
                )),
            }
        }
    }
}

/// Safety net, not the primary fix — the packed image / live overlayer
/// this deploys from should simply not contain any installer at all
/// (it's a live-session-only tool, never part of what actually ships on
/// the installed disk). Nothing in `deploy_squash`/`deploy_erofs`/
/// `deploy_rsync`/`deploy_keep_squash` excludes anything from the source,
/// though — whatever's in the source image gets cloned onto the target
/// verbatim — so if an installer ever does end up packed into that
/// image, this removes it from the freshly-deployed target after the
/// fact rather than leaving a live-installer binary sitting on a
/// finished, installed system. Best-effort: a missing file here isn't a
/// deploy failure, it just means there was nothing to clean up.
fn remove_installer_from_target(root: &Path, progress: &mut ProgressState) {
    let hub = syshub(root);
    let sys = zaisys(root);
    let paths = [
        // carve itself lives in zaisys, not syshub -- syshub stays
        // graphical-only/immutable, never carries install-only content.
        sys.join("bin/carve"),
        sys.join("lib/systemd/system/carve-install.service"),
        sys.join("lib/systemd/system/carve-install.target"),
        hub.join("bin/zainium-installer"),
        hub.join("share/applications/tech.zainiumdynamics.Installer.desktop"),
        hub.join("share/icons/hicolor/scalable/apps/tech.zainiumdynamics.Installer.svg"),
    ];
    for path in paths {
        if !path.exists() {
            continue;
        }
        match fs::remove_file(&path) {
            Ok(()) => progress.append_log(format!(
                "deploy: removed installer file from target ({})",
                path.display()
            )),
            Err(e) => progress.append_log(format!(
                "deploy: WARN — could not remove installer file from target ({}): {e}",
                path.display()
            )),
        }
    }
}

fn deploy_missing(plan: &InstallPlan, progress: &mut ProgressState) -> Result<String, String> {
    if plan.dry_run {
        progress.append_log(
            "deploy: dry-run — no squash/overlayer on this host yet; skeleton only (not a fake OS)",
        );
        progress.append_log(
            "deploy: when ready set ZAINIUM_SQUASH or ZAINIUM_OVERLAYER and re-run",
        );
        return Ok("skipped deploy (no source assets)".into());
    }
    Err(
        "no OS source: provide zairoot.img/zairoot.squash (ZAINIUM_SQUASH) or overlayer (ZAINIUM_OVERLAYER)"
            .into(),
    )
}

/// Expand the packed root image (EROFS or SquashFS, by extension) onto the
/// target partition. Both tools extract in userspace — no mount/root
/// capability needed beyond normal disk-write access to `root`.
fn deploy_packed_image(
    image: &Path,
    root: &Path,
    progress: &mut ProgressState,
) -> Result<String, String> {
    match image_format(Some(image)) {
        "erofs" => deploy_erofs(image, root, progress),
        _ => deploy_squash(image, root, progress),
    }
}

fn deploy_erofs(
    image: &Path,
    root: &Path,
    progress: &mut ProgressState,
) -> Result<String, String> {
    progress.append_log(format!("deploy: fsck.erofs --extract {}", image.display()));
    if !which("fsck.erofs") {
        return Err("fsck.erofs not found in PATH (erofs-utils)".into());
    }
    fs::create_dir_all(root).map_err(|e| e.to_string())?;
    let status = Command::new("fsck.erofs")
        .arg(format!("--extract={}", root.display()))
        .arg("--overwrite")
        .arg(image)
        .status()
        .map_err(|e| e.to_string())?;
    if !status.success() {
        return Err(format!("fsck.erofs --extract failed for {}", image.display()));
    }
    ensure_zexlib(root)?;
    Ok(format!("fsck.erofs --extract {}", image.display()))
}

fn deploy_squash(
    squash: &Path,
    root: &Path,
    progress: &mut ProgressState,
) -> Result<String, String> {
    progress.append_log(format!("deploy: unsquashfs {}", squash.display()));
    if !which("unsquashfs") {
        return Err("unsquashfs not found in PATH".into());
    }
    let status = Command::new("unsquashfs")
        .args(["-f", "-d"])
        .arg(root)
        .arg(squash)
        .status()
        .map_err(|e| e.to_string())?;
    if !status.success() {
        return Err(format!("unsquashfs failed for {}", squash.display()));
    }
    ensure_zexlib(root)?;
    Ok(format!("unsquashfs {}", squash.display()))
}

fn deploy_rsync(
    live: &Path,
    root: &Path,
    plan: &InstallPlan,
    progress: &mut ProgressState,
) -> Result<String, String> {
    if plan.dry_run && std::env::var("ZAINIUM_DRY_RUN_FULL").ok().as_deref() != Some("1") {
        progress.append_log(
            "deploy: dry-run without ZAINIUM_DRY_RUN_FULL=1 — skip multi-GB rsync (set it to copy for real)",
        );
        progress.append_log(format!("deploy: would rsync {} → target overlayer", live.display()));
        return Ok("dry-run skip full rsync".into());
    }

    if !live.is_dir() {
        return Err(format!("overlayer not a directory: {}", live.display()));
    }

    let dest = root.join("overlayer");
    fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
    progress.append_log(format!(
        "deploy: rsync {} → {}",
        live.display(),
        dest.display()
    ));

    if !which("rsync") {
        return Err("rsync not found in PATH".into());
    }
    let status = Command::new("rsync")
        .args(["-aHAX", "--info=progress2"])
        .arg(format!("{}/", live.display()))
        .arg(format!("{}/", dest.display()))
        .status()
        .map_err(|e| e.to_string())?;
    if !status.success() {
        return Err("rsync failed".into());
    }
    ensure_zexlib(root)?;
    if !syshub(root).is_dir() {
        return Err("after rsync, overlayer/syshub missing — source tree incomplete".into());
    }
    Ok(format!("rsync from {}", live.display()))
}

fn deploy_keep_squash(
    squash: &Path,
    root: &Path,
    progress: &mut ProgressState,
) -> Result<String, String> {
    let dest = zaisys(root).join(
        squash
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("zairoot.squash"),
    );
    fs::create_dir_all(zaisys(root)).map_err(|e| e.to_string())?;
    progress.append_log(format!(
        "deploy: copy squash {} → {}",
        squash.display(),
        dest.display()
    ));
    fs::copy(squash, &dest).map_err(|e| e.to_string())?;
    ensure_zexlib(root)?;
    Ok(format!("kept squash at {}", dest.display()))
}

fn ensure_zexlib(root: &Path) -> Result<(), String> {
    fs::create_dir_all(zexlib_union(root)).map_err(|e| e.to_string())?;
    fs::create_dir_all(zexlib_work(root)).map_err(|e| e.to_string())?;
    fs::create_dir_all(zexlib_registry(root)).map_err(|e| e.to_string())?;
    Ok(())
}

fn which(bin: &str) -> bool {
    Command::new("sh")
        .args(["-c", &format!("command -v {bin} >/dev/null 2>&1")])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}
