// Copyright (c) 2026 Zainiumdynamics. All rights reserved.
// Designed for Zainium OS by Zainiumdynamics — https://zainiumdynamics.tech

use std::path::{Path, PathBuf};

use crate::model::InstallPlan;

use super::config::InstallerConfig;

/// Physical target root (mounted install root or dry-run directory).
pub fn target_root(plan: &InstallPlan) -> PathBuf {
    if plan.dry_run {
        plan.dry_run_root.clone()
    } else {
        InstallerConfig::from_env().install_mount
    }
}

pub fn overlayer(root: &Path) -> PathBuf {
    root.join("overlayer")
}

pub fn syshub(root: &Path) -> PathBuf {
    overlayer(root).join("syshub")
}

pub fn zaisys(root: &Path) -> PathBuf {
    overlayer(root).join("zaisys")
}

pub fn zexlib_union(root: &Path) -> PathBuf {
    overlayer(root).join("zexlib/union")
}

pub fn zexlib_work(root: &Path) -> PathBuf {
    overlayer(root).join("zexlib/work")
}

pub fn zexlib_registry(root: &Path) -> PathBuf {
    overlayer(root).join("zexlib/registry/installed")
}

pub fn quantra_services_dir(root: &Path) -> PathBuf {
    syshub(root).join("etc/quantra-system/services")
}

pub fn quantra_enabled_dir(root: &Path) -> PathBuf {
    syshub(root).join("etc/quantra-system/enabled")
}
