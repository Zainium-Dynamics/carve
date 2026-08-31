// Copyright (c) 2026 Zainiumdynamics. All rights reserved.
// Designed for Zainium OS by Zainiumdynamics — https://zainiumdynamics.tech
//
// Headless, non-interactive install path. Zero prompts — every value
// comes from the TOML config passed via `--config path.toml`, or the
// install is refused outright. See README.md for the full schema.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::{
    backend,
    model::{InstallPlan, JobStatus, ProgressState},
    tzdata,
};

#[derive(Deserialize)]
struct FileConfig {
    system: SystemCfg,
    user: UserCfg,
    #[serde(default)]
    disk: DiskCfg,
    #[serde(default)]
    install: InstallCfg,
}

#[derive(Deserialize, Default)]
struct SystemCfg {
    locale: Option<String>,
    keyboard: Option<String>,
    timezone: Option<String>,
    hostname: Option<String>,
}

#[derive(Deserialize)]
struct UserCfg {
    full_name: String,
    username: String,
    /// Plaintext password, only for throwaway/CI use — prefer `password_env`
    /// so real credentials never sit in a config file baked into an ISO.
    password: Option<String>,
    /// Name of an environment variable holding the password (recommended).
    password_env: Option<String>,
}

#[derive(Deserialize, Default)]
struct DiskCfg {
    /// "largest" (default) | "only" (error unless exactly one candidate) | an explicit /dev/... path
    select: Option<String>,
}

#[derive(Deserialize, Default)]
struct InstallCfg {
    /// Default false — a config you deliberately point --config at is
    /// assumed to mean a real install. Set true explicitly to test the
    /// pipeline without touching a disk.
    dry_run: Option<bool>,
    /// Default false — an unattended install must opt in explicitly to
    /// rebooting the machine when it's done.
    reboot_after: Option<bool>,
}

pub fn run(config_path: &Path) -> i32 {
    let raw = match std::fs::read_to_string(config_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("cannot read {}: {e}", config_path.display());
            return 1;
        }
    };
    let cfg: FileConfig = match toml::from_str(&raw) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("invalid config {}: {e}", config_path.display());
            return 1;
        }
    };

    let mut plan = InstallPlan::default();
    plan.dry_run = cfg.install.dry_run.unwrap_or(false);

    if let Some(v) = cfg.system.locale {
        plan.locale = v;
    }
    if let Some(v) = cfg.system.keyboard {
        plan.keyboard = v;
    }
    if let Some(v) = cfg.system.hostname {
        plan.hostname = v;
    }
    if let Some(v) = cfg.system.timezone {
        if !tzdata::zone_exists(&v) {
            eprintln!(
                "config error: timezone '{v}' isn't a real zone under /usr/share/zoneinfo \
                 (or ZAINIUM_ZONEINFO) on this medium — refusing to guess"
            );
            return 1;
        }
        plan.timezone = v;
    }

    plan.full_name = cfg.user.full_name;
    plan.username = cfg.user.username;
    plan.password = match resolve_password(cfg.user.password, cfg.user.password_env) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("config error: {e}");
            return 1;
        }
    };

    if let Err(field) = plan.validate_user() {
        eprintln!("config error: invalid {field} (username: lowercase [a-z0-9_-], starts with a letter; password: 4+ chars)");
        return 1;
    }

    match resolve_disk(cfg.disk.select.as_deref(), plan.dry_run) {
        Ok(disk) => plan.disk = disk,
        Err(e) => {
            eprintln!("config error: {e}");
            return 1;
        }
    }

    let mut progress = ProgressState::pipeline();
    let result = backend::run_install(&mut plan, &mut progress, |p| {
        if let Some(job) = p.jobs.iter().rev().find(|j| j.status != JobStatus::Pending) {
            let mark = match job.status {
                JobStatus::Running => "…",
                JobStatus::Done => "✓",
                JobStatus::Failed => "✗",
                JobStatus::Skipped => "–",
                JobStatus::Pending => " ",
            };
            eprintln!("carve: {mark} {:<10} {}", job.id, job.detail);
        }
    });

    match result {
        Ok(()) => {
            eprintln!("carve: install finished successfully");
            if !plan.dry_run && cfg.install.reboot_after.unwrap_or(false) {
                eprintln!("carve: reboot_after=true — rebooting");
                if let Err(e) = backend::reboot() {
                    eprintln!("carve: reboot failed: {e}");
                    return 1;
                }
            }
            0
        }
        Err(e) => {
            eprintln!("carve: install FAILED: {e}");
            eprintln!("{}", progress.log);
            1
        }
    }
}

fn resolve_password(direct: Option<String>, env_name: Option<String>) -> Result<String, String> {
    if let Some(name) = env_name {
        return std::env::var(&name)
            .map_err(|_| format!("password_env names '{name}' but that variable isn't set"));
    }
    direct.ok_or_else(|| "user.password or user.password_env is required".to_string())
}

fn resolve_disk(
    select: Option<&str>,
    dry_run: bool,
) -> Result<Option<crate::model::DiskCandidate>, String> {
    // Under dry_run the partition/format jobs never touch plan.disk anyway
    // (they short-circuit to their own stub paths) — still resolve it here
    // so the dry-run log honestly reflects what a real run would pick.
    let usable: Vec<_> = backend::disk::list_disks()
        .into_iter()
        .filter(|d| !d.is_live_medium)
        .collect();
    pick_disk(usable, select.unwrap_or("largest"), dry_run)
}

fn pick_disk(
    candidates: Vec<crate::model::DiskCandidate>,
    select: &str,
    dry_run: bool,
) -> Result<Option<crate::model::DiskCandidate>, String> {
    match select {
        explicit if explicit.starts_with('/') => candidates
            .into_iter()
            .find(|d| d.path == explicit)
            .map(Some)
            .ok_or_else(|| format!("disk '{explicit}' not found (or is the live medium)")),
        "only" => match candidates.len() {
            1 => Ok(Some(candidates.into_iter().next().unwrap())),
            0 => Err("no usable disks found".to_string()),
            n => Err(format!(
                "disk.select = \"only\" but {n} candidate disks found — be explicit"
            )),
        },
        other => {
            if other != "largest" {
                eprintln!("carve: unknown disk.select '{other}' — falling back to \"largest\"");
            }
            if candidates.is_empty() {
                if dry_run {
                    return Ok(None);
                }
                return Err("no usable disks found".to_string());
            }
            Ok(candidates.into_iter().max_by_key(|d| d.size_bytes))
        }
    }
}

pub fn default_config_path() -> PathBuf {
    PathBuf::from("/overlayer/zaisys/carve/autoinstall.toml")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::DiskCandidate;

    fn disk(path: &str, gib: u64) -> DiskCandidate {
        DiskCandidate {
            path: path.to_string(),
            model: "test".to_string(),
            size_bytes: gib * 1024 * 1024 * 1024,
            is_live_medium: false,
        }
    }

    #[test]
    fn pick_disk_explicit_path_match_and_no_match() {
        let candidates = vec![disk("/dev/sda", 100), disk("/dev/sdb", 200)];
        assert_eq!(
            pick_disk(candidates.clone(), "/dev/sdb", false)
                .unwrap()
                .unwrap()
                .path,
            "/dev/sdb"
        );
        assert!(pick_disk(candidates, "/dev/nvme0n1", false).is_err());
    }

    #[test]
    fn pick_disk_only_requires_exactly_one_candidate() {
        assert!(pick_disk(vec![], "only", false).is_err());
        assert_eq!(
            pick_disk(vec![disk("/dev/sda", 100)], "only", false)
                .unwrap()
                .unwrap()
                .path,
            "/dev/sda"
        );
        assert!(pick_disk(vec![disk("/dev/sda", 100), disk("/dev/sdb", 200)], "only", false).is_err());
    }

    #[test]
    fn pick_disk_largest_picks_max_size() {
        let candidates = vec![disk("/dev/sda", 100), disk("/dev/sdb", 500), disk("/dev/sdc", 250)];
        assert_eq!(
            pick_disk(candidates, "largest", false).unwrap().unwrap().path,
            "/dev/sdb"
        );
    }

    #[test]
    fn pick_disk_unknown_selector_falls_back_to_largest() {
        let candidates = vec![disk("/dev/sda", 100), disk("/dev/sdb", 500)];
        assert_eq!(
            pick_disk(candidates, "bogus", false).unwrap().unwrap().path,
            "/dev/sdb"
        );
    }

    #[test]
    fn pick_disk_empty_ok_under_dry_run_error_otherwise() {
        assert_eq!(pick_disk(vec![], "largest", true).unwrap(), None);
        assert!(pick_disk(vec![], "largest", false).is_err());
    }
}
