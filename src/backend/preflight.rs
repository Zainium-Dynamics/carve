// Copyright (c) 2026 Zainiumdynamics. All rights reserved.
// Designed for Zainium OS by Zainiumdynamics — https://zainiumdynamics.tech

use std::fs;

use crate::model::{InstallPlan, ProgressState};

use super::{
    assets::{discover, write_report},
    config::InstallerConfig,
    paths::target_root,
};

pub fn job_preflight(plan: &mut InstallPlan, progress: &mut ProgressState) -> Result<String, String> {
    plan.validate_user()
        .map_err(|_| "invalid username/password".to_string())?;

    if plan.hostname.trim().is_empty() {
        return Err("hostname required".into());
    }

    if !plan.dry_run {
        if plan.disk.as_ref().is_none_or(|d| d.is_live_medium) {
            return Err("invalid or live target disk".into());
        }
        // `disk::job_partition`/`job_format`/`job_mount` each wrap their own
        // command through `elevate` individually, so they work fine from a
        // non-root process — but `deploy::job_seed_tree`/`job_deploy_os`
        // and `user::job_create_user` write directly into the mounted
        // target root with bare `fs::` calls, no subprocess to wrap in
        // `elevate` at all. A freshly `mkfs`'d/mounted root is root:root
        // 0755 by default, so those writes fail outright unless this
        // process itself is already root. Catching that here, before any
        // disk is touched, turns a confusing mid-install permission error
        // into a clear one at the very first preflight check.
        if !super::elevate_cmd::is_root() {
            return Err(
                "real install blocked: must run as root (relaunch via `elevate carve`) \
                 — deploying the OS and creating the user write directly into the mounted target \
                 root, which needs this process itself to be root, not just individual commands \
                 elevated one at a time"
                    .into(),
            );
        }
        let cfg = InstallerConfig::from_env();
        let assets = discover(&cfg);
        let missing = assets.missing_for_real_install();
        if !missing.is_empty() {
            return Err(format!(
                "real install blocked; missing assets: {}",
                missing.join("; ")
            ));
        }
    }

    let root = target_root(plan);
    if plan.dry_run {
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).map_err(|e| e.to_string())?;
        progress.append_log(format!("preflight: dry-run root {}", root.display()));
    }

    let cfg = InstallerConfig::from_env();
    let assets = discover(&cfg);
    let report_path = root.join("ASSET-REPORT.toml");
    write_report(&report_path, &assets)?;
    progress.append_log(format!("preflight: asset report → {}", report_path.display()));
    for n in &assets.notes {
        progress.append_log(format!("preflight: note: {n}"));
    }

    progress.append_log(format!(
        "preflight: host={} user={} mode={:?} dry_run={} os_source={} boot_payload={}",
        plan.hostname,
        plan.username,
        plan.mode,
        plan.dry_run,
        assets.has_os_source(),
        assets.has_boot_payload()
    ));
    Ok("ok".into())
}
