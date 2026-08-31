// Copyright (c) 2026 Zainiumdynamics. All rights reserved.
// Designed for Zainium OS by Zainiumdynamics — https://zainiumdynamics.tech
//
// Timezones, discovered — zero-hardcode. Every zone this module offers
// comes from actually walking the live IANA tzdata tree at
// `/usr/share/zoneinfo` (or `ZAINIUM_ZONEINFO` override): whatever the
// live/install medium's `tzdata` package actually ships is exactly what's
// offered, no baked-in list of "5 popular cities" that goes stale or
// leaves countries out. Same tree glibc/musl `localtime()` reads.

use std::{
    fs,
    path::{Path, PathBuf},
};

/// Real region directories under zoneinfo root. `posix`/`right` are the
/// same data duplicated (plain vs. leap-second variants), `Factory` is a
/// deliberate dummy placeholder zone, `SystemV` is legacy POSIX names —
/// none of these are real "Region/City" choices for a human.
const SKIP_TOP: &[&str] = &["posix", "right", "Factory", "SystemV"];

fn zoneinfo_root() -> PathBuf {
    std::env::var_os("ZAINIUM_ZONEINFO")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/usr/share/zoneinfo"))
}

/// True if the live system actually has usable tzdata to offer.
pub fn available() -> bool {
    zoneinfo_root().is_dir()
}

/// Top-level regions that contain real "Region/City" zones (Africa,
/// America, Asia, Europe, …), sorted. Empty if tzdata isn't installed on
/// this medium — callers should fall back to a plain UTC default and say
/// so honestly, not invent region names.
pub fn regions() -> Vec<String> {
    let root = zoneinfo_root();
    let Ok(rd) = fs::read_dir(&root) else {
        return Vec::new();
    };
    let mut out: Vec<String> = rd
        .flatten()
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|name| !SKIP_TOP.contains(&name.as_str()) && name != "Etc")
        .collect();
    out.sort();
    out
}

/// Standalone top-level zones that aren't inside a region dir (UTC and a
/// handful of legacy single-name zones like "Japan", "Turkey"). Also
/// discovered, not hardcoded.
pub fn standalone_zones() -> Vec<String> {
    let root = zoneinfo_root();
    let Ok(rd) = fs::read_dir(&root) else {
        return Vec::new();
    };
    let mut out: Vec<String> = rd
        .flatten()
        .filter(|e| e.path().is_file())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    out.sort();
    out
}

/// Zone names inside one region, relative to the region (e.g. "Karachi",
/// or "Argentina/Buenos_Aires" for regions with sub-directories),
/// discovered by walking the real directory tree.
pub fn zones_in_region(region: &str) -> Vec<String> {
    let root = zoneinfo_root();
    let region_dir = root.join(region);
    let mut out = Vec::new();
    walk(&region_dir, "", &mut out);
    out.sort();
    out
}

fn walk(dir: &Path, prefix: &str, out: &mut Vec<String>) {
    let Ok(rd) = fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let path = e.path();
        let name = e.file_name().to_string_lossy().into_owned();
        let rel = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        if path.is_dir() {
            walk(&path, &rel, out);
        } else if path.is_file() {
            out.push(rel);
        }
    }
}

/// Validate a full zone name (e.g. "Asia/Karachi", "UTC") against the real
/// tzdata tree — never against a hardcoded list. Used by both the wizard
/// (defense in depth) and autoinstall TOML parsing (a config file can name
/// any zone; only the live tzdata tree gets to say whether it's real).
pub fn zone_exists(zone: &str) -> bool {
    if zone.is_empty() || zone.contains("..") {
        return false;
    }
    zoneinfo_root().join(zone).is_file()
}

/// Full "Region/City" identifiers across every real region, flattened —
/// used when a caller wants one combined list instead of the two-step
/// region → city picker.
pub fn all_zones() -> Vec<String> {
    let mut out = standalone_zones();
    for region in regions() {
        for city in zones_in_region(&region) {
            out.push(format!("{region}/{city}"));
        }
    }
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skip_list_excludes_non_region_dirs() {
        assert!(SKIP_TOP.contains(&"posix"));
        assert!(SKIP_TOP.contains(&"right"));
        assert!(SKIP_TOP.contains(&"Factory"));
    }

    #[test]
    fn zone_exists_rejects_path_traversal() {
        assert!(!zone_exists("../../etc/passwd"));
        assert!(!zone_exists(""));
    }

    /// No other test in this crate touches `ZAINIUM_ZONEINFO`, so pointing
    /// it at a throwaway fixture tree for the duration of this test is
    /// safe in practice.
    fn with_fake_zoneinfo(f: impl FnOnce()) {
        let root = std::env::temp_dir().join(format!("carve-tz-test-{}", std::process::id()));
        fs::create_dir_all(root.join("Asia")).unwrap();
        fs::write(root.join("Asia/Karachi"), b"fake-tzfile").unwrap();
        fs::create_dir_all(root.join("America/Argentina")).unwrap();
        fs::write(root.join("America/Argentina/Buenos_Aires"), b"fake-tzfile").unwrap();
        fs::write(root.join("UTC"), b"fake-tzfile").unwrap();
        fs::create_dir_all(root.join("posix")).unwrap(); // must be skipped

        let prev = std::env::var_os("ZAINIUM_ZONEINFO");
        unsafe { std::env::set_var("ZAINIUM_ZONEINFO", &root) };

        f();

        match prev {
            Some(v) => unsafe { std::env::set_var("ZAINIUM_ZONEINFO", v) },
            None => unsafe { std::env::remove_var("ZAINIUM_ZONEINFO") },
        }
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn walks_a_fake_tzdata_tree_honestly() {
        with_fake_zoneinfo(|| {
            assert!(available());

            let regions = regions();
            assert!(regions.contains(&"Asia".to_string()));
            assert!(regions.contains(&"America".to_string()));
            assert!(!regions.contains(&"posix".to_string()));

            assert_eq!(zones_in_region("Asia"), vec!["Karachi".to_string()]);
            assert_eq!(
                zones_in_region("America"),
                vec!["Argentina/Buenos_Aires".to_string()]
            );

            assert!(standalone_zones().contains(&"UTC".to_string()));

            assert!(zone_exists("Asia/Karachi"));
            assert!(zone_exists("America/Argentina/Buenos_Aires"));
            assert!(zone_exists("UTC"));
            assert!(!zone_exists("Asia/Nowhere"));

            let all = all_zones();
            assert!(all.contains(&"Asia/Karachi".to_string()));
            assert!(all.contains(&"America/Argentina/Buenos_Aires".to_string()));
        });
    }
}
