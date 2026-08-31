// Copyright (c) 2026 Zainiumdynamics. All rights reserved.
// Designed for Zainium OS by Zainiumdynamics — https://zainiumdynamics.tech

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// How the OS image is applied to the target root.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum InstallMode {
    /// Expand the packed root image (`zairoot.img` EROFS, or legacy
    /// `zairoot.squash`) via `fsck.erofs --extract` / `unsquashfs` (recommended v1).
    #[default]
    ExpandSquash,
    /// rsync live overlayer tree (dev / when the packed image is missing).
    RsyncLiveTree,
    /// Keep the packed image on disk + `loop=`/`loopfstype=` (v2) — quantra-ramfs's
    /// cmdline.rs already supports this; eclipse-iso-builder's generated
    /// eclipse.conf sets it automatically.
    KeepSquash,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum PartitionMode {
    #[default]
    WholeDisk,
    Manual,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DiskCandidate {
    pub path: String,
    pub model: String,
    pub size_bytes: u64,
    pub is_live_medium: bool,
}

impl DiskCandidate {
    pub fn label(&self) -> String {
        let gib = self.size_bytes as f64 / (1024.0 * 1024.0 * 1024.0);
        format!("{} — {} ({:.1} GiB)", self.path, self.model, gib)
    }
}

/// Serializable install plan built by the wizard / autoinstall TOML.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InstallPlan {
    pub locale: String,
    pub keyboard: String,
    /// IANA zone name (e.g. "Asia/Karachi") — validated against the live
    /// `/usr/share/zoneinfo` tree at selection time (see `crate::tzdata`),
    /// never a hardcoded list baked into this binary.
    pub timezone: String,
    pub disk: Option<DiskCandidate>,
    pub partition_mode: PartitionMode,
    pub full_name: String,
    pub username: String,
    pub password: String,
    pub hostname: String,
    pub mode: InstallMode,
    /// When true, never touch real block devices — write under dry_run_root.
    pub dry_run: bool,
    pub dry_run_root: PathBuf,
    pub squash_path: Option<PathBuf>,
    pub live_overlayer: Option<PathBuf>,
    pub zaisys_path: Option<PathBuf>,
    /// EFI System Partition size in MiB (default 512).
    pub efi_size_mib: u64,
    /// Filled after partition: e.g. /dev/sda1
    #[serde(default)]
    pub efi_part: Option<String>,
    /// Filled after partition: e.g. /dev/sda2
    #[serde(default)]
    pub root_part: Option<String>,
    /// Filled after format (blkid of root part)
    #[serde(default)]
    pub root_uuid: Option<String>,
}

impl Default for InstallPlan {
    fn default() -> Self {
        let dry_run_root = std::env::var_os("ZAINIUM_DRY_RUN_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/tmp/zainium-install-test"));
        let efi_size_mib = std::env::var("ZAINIUM_EFI_SIZE_MIB")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(512);
        Self {
            locale: std::env::var("ZAINIUM_LOCALE").unwrap_or_else(|_| "en_US.UTF-8".into()),
            keyboard: std::env::var("ZAINIUM_KEYBOARD").unwrap_or_else(|_| "us".into()),
            timezone: std::env::var("ZAINIUM_TIMEZONE").unwrap_or_else(|_| "UTC".into()),
            disk: None,
            partition_mode: PartitionMode::WholeDisk,
            full_name: String::new(),
            username: String::new(),
            password: String::new(),
            hostname: std::env::var("ZAINIUM_HOSTNAME").unwrap_or_else(|_| "zainium".into()),
            mode: InstallMode::ExpandSquash,
            dry_run: true,
            dry_run_root,
            squash_path: std::env::var_os("ZAINIUM_SQUASH").map(PathBuf::from),
            live_overlayer: std::env::var_os("ZAINIUM_OVERLAYER").map(PathBuf::from),
            zaisys_path: std::env::var_os("ZAINIUM_ZAISYS").map(PathBuf::from),
            efi_size_mib,
            efi_part: None,
            root_part: None,
            root_uuid: None,
        }
    }
}

/// Lowercase, starts with a letter, `[a-z0-9_-]` only. Shared by
/// `InstallPlan::validate_user` and the wizard, which needs to check the
/// username shape before a password even exists to validate alongside it.
pub fn valid_username(s: &str) -> bool {
    s.chars().next().is_some_and(|c| c.is_ascii_lowercase())
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
}

impl InstallPlan {
    pub fn validate_user(&self) -> Result<(), &'static str> {
        if !valid_username(&self.username) {
            return Err("error-username");
        }
        if self.password.is_empty() || self.password.len() < 4 {
            return Err("error-username");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_username_accepts_lowercase_letters_digits_dash_underscore() {
        assert!(valid_username("alizain"));
        assert!(valid_username("a"));
        assert!(valid_username("ali-zain_2"));
    }

    #[test]
    fn valid_username_rejects_empty_uppercase_digit_start_and_bad_chars() {
        assert!(!valid_username(""));
        assert!(!valid_username("Alizain"));
        assert!(!valid_username("2ali"));
        assert!(!valid_username("ali zain"));
        assert!(!valid_username("ali.zain"));
    }

    #[test]
    fn validate_user_enforces_password_length() {
        let mut plan = InstallPlan {
            username: "alizain".into(),
            ..InstallPlan::default()
        };
        plan.password = "abc".into();
        assert!(plan.validate_user().is_err());
        plan.password = "abcd".into();
        assert!(plan.validate_user().is_ok());
    }
}
