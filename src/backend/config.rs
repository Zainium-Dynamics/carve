// Copyright (c) 2026 Zainiumdynamics. All rights reserved.
// Designed for Zainium OS by Zainiumdynamics — https://zainiumdynamics.tech
//
// Paths from environment / discovery only. Boot menu text is never authored here.

use std::path::PathBuf;

/// Runtime configuration for the installer.
#[derive(Clone, Debug)]
pub struct InstallerConfig {
    pub zairoot: Option<PathBuf>,
    pub overlayer: Option<PathBuf>,
    pub zaisys: Option<PathBuf>,
    pub squash: Option<PathBuf>,
    pub install_mount: PathBuf,
    pub root_fstype: String,
    /// Optional override: live limine.conf you prepared (default: zaisys/limine/limine.conf).
    pub limine_live_conf: Option<PathBuf>,
    /// Optional override: install-time conf for installer to deploy.
    pub limine_install_conf: Option<PathBuf>,
    /// Quantra services to enable markers for (only if .toml already on target).
    pub quantra_enable: Vec<String>,
}

impl Default for InstallerConfig {
    fn default() -> Self {
        Self::from_env()
    }
}

impl InstallerConfig {
    pub fn from_env() -> Self {
        // Empty list = enable nothing unless ZAINIUM_QUANTRA_ENABLE set
        // (no hardcoded service names forced). Default list only if unset entirely.
        let quantra_enable = env_list("ZAINIUM_QUANTRA_ENABLE").unwrap_or_else(|| {
            // Must match service TOML `name` under quantra-system/services/
            vec![
                "quantra-netd".into(),
                "quantra-net".into(),
                "quantra-logind".into(),
                "console-shell".into(),
                "graphical-launcher".into(),
            ]
        });

        Self {
            zairoot: env_path("ZAINIUM_ZAIROOT"),
            overlayer: env_path("ZAINIUM_OVERLAYER"),
            zaisys: env_path("ZAINIUM_ZAISYS"),
            squash: env_path("ZAINIUM_SQUASH"),
            install_mount: env_path("ZAINIUM_INSTALL_MOUNT")
                .unwrap_or_else(|| PathBuf::from("/mnt/install")),
            root_fstype: env_string("ZAINIUM_ROOT_FSTYPE").unwrap_or_else(|| "ext4".into()),
            limine_live_conf: env_path("ZAINIUM_LIMINE_LIVE_CONF"),
            limine_install_conf: env_path("ZAINIUM_LIMINE_INSTALL_CONF"),
            quantra_enable,
        }
    }
}

fn env_path(key: &str) -> Option<PathBuf> {
    std::env::var_os(key)
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
}

fn env_string(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|s| !s.is_empty())
}

fn env_list(key: &str) -> Option<Vec<String>> {
    env_string(key).map(|s| {
        s.split(',')
            .map(str::trim)
            .filter(|x| !x.is_empty())
            .map(str::to_string)
            .collect()
    })
}
