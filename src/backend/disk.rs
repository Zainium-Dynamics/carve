// Copyright (c) 2026 Zainiumdynamics. All rights reserved.
// Designed for Zainium OS by Zainiumdynamics — https://zainiumdynamics.tech
//
// GPT whole-disk: ESP (FAT32) + ROOT (ext4). Privileged ops via elevate (no sudo).

use std::{fs, path::Path, process::Command, thread, time::Duration};

use crate::model::{DiskCandidate, InstallPlan, PartitionMode, ProgressState};

use super::{
    elevate_cmd::{is_root, run_root_ok, run_root_stdout},
    limine::blkid_uuid,
    paths::target_root,
};

/// Enumerate real disks via lsblk. Marks live medium when possible.
pub fn list_disks() -> Vec<DiskCandidate> {
    let mut out = Vec::new();
    let Ok(output) = Command::new("lsblk")
        .args(["-b", "-dn", "-o", "NAME,SIZE,MODEL,TYPE"])
        .output()
    else {
        return out;
    };
    if !output.status.success() {
        return out;
    }
    let live_devs = live_block_devices();
    let text = String::from_utf8_lossy(&output.stdout);
    for line in text.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 3 {
            continue;
        }
        if parts.last().copied() != Some("disk") {
            continue;
        }
        let name = parts[0];
        let size: u64 = parts[1].parse().unwrap_or(0);
        let model = if parts.len() > 3 {
            parts[2..parts.len() - 1].join(" ")
        } else {
            String::new()
        };
        let path = format!("/dev/{name}");
        let is_live = live_devs.iter().any(|d| d == &path || path.starts_with(d));
        out.push(DiskCandidate {
            path,
            model: if model.is_empty() {
                "disk".into()
            } else {
                model
            },
            size_bytes: size,
            is_live_medium: is_live,
        });
    }
    out
}

/// Devices currently backing root / live mounts (best-effort).
fn live_block_devices() -> Vec<String> {
    let mut v = Vec::new();
    if let Ok(out) = Command::new("findmnt")
        .args(["-n", "-o", "SOURCE", "/"])
        .output()
    {
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if let Some(d) = normalize_block_dev(&s) {
                v.push(d);
            }
        }
    }
    // Common live mounts
    for mp in ["/run/live", "/lib/live/mount/medium", "/cdrom"] {
        if let Ok(out) = Command::new("findmnt")
            .args(["-n", "-o", "SOURCE", mp])
            .output()
        {
            if out.status.success() {
                let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if let Some(d) = normalize_block_dev(&s) {
                    v.push(d);
                }
            }
        }
    }
    v
}

fn normalize_block_dev(source: &str) -> Option<String> {
    // /dev/sda2 → /dev/sda ; /dev/nvme0n1p2 → /dev/nvme0n1
    let s = source.trim();
    if !s.starts_with("/dev/") {
        return None;
    }
    let base = s.trim_end_matches(|c: char| c.is_ascii_digit());
    let base = base.strip_suffix('p').unwrap_or(base);
    if base == "/dev/" || base.is_empty() {
        Some(s.to_string())
    } else {
        Some(base.to_string())
    }
}

/// Build partition path for Nth partition (1-based).
pub fn partition_path(disk: &str, n: u32) -> String {
    let name = disk.rsplit('/').next().unwrap_or(disk);
    if name.starts_with("nvme")
        || name.starts_with("mmcblk")
        || name.starts_with("loop")
        || name.starts_with("nbd")
    {
        format!("{disk}p{n}")
    } else {
        format!("{disk}{n}")
    }
}

pub fn job_partition(plan: &mut InstallPlan, progress: &mut ProgressState) -> Result<String, String> {
    if plan.dry_run {
        // Optional: real loop file partition when ZAINIUM_DRY_RUN_LOOP=1 and root/elevate
        if std::env::var("ZAINIUM_DRY_RUN_LOOP").ok().as_deref() == Some("1") {
            return partition_loop_dry_run(plan, progress);
        }
        progress.append_log("partition: dry-run — skip wipe (set ZAINIUM_DRY_RUN_LOOP=1 for loop test)");
        plan.efi_part = Some("dry-run-efi".into());
        plan.root_part = Some("dry-run-root".into());
        return Ok("dry-run skip partition".into());
    }

    let disk = plan
        .disk
        .as_ref()
        .ok_or_else(|| "no disk selected".to_string())?
        .clone();
    if disk.is_live_medium {
        return Err("refusing to partition live medium".into());
    }
    if matches!(plan.partition_mode, PartitionMode::Manual) {
        return Err("manual partition mode not implemented — use whole-disk".into());
    }
    if disk.size_bytes > 0 && disk.size_bytes < 4 * 1024 * 1024 * 1024 {
        return Err("disk too small (need ≥ 4 GiB)".into());
    }

    progress.append_log(format!(
        "partition: GPT on {} (ESP {} MiB + ROOT) elevate/root={}",
        disk.path,
        plan.efi_size_mib,
        is_root()
    ));

    // Wipe signatures
    let _ = run_root_ok("wipefs", &["-a", &disk.path]);

    // sfdisk GPT: type U = EFI, L = Linux filesystem
    let script = format!("label: gpt\n,{}M,U\n,,L\n", plan.efi_size_mib);
    // Prefer feeding script via stdin through elevate
    write_sfdisk(&disk.path, &script, progress)?;

    // Wait for udev
    let _ = run_root_ok("udevadm", &["settle"]);
    thread::sleep(Duration::from_millis(500));
    let _ = run_root_ok("partprobe", &[&disk.path]);
    thread::sleep(Duration::from_millis(300));

    let efi = partition_path(&disk.path, 1);
    let root = partition_path(&disk.path, 2);
    wait_for_node(&efi, 50)?;
    wait_for_node(&root, 50)?;

    plan.efi_part = Some(efi.clone());
    plan.root_part = Some(root.clone());
    progress.append_log(format!("partition: efi={efi} root={root}"));
    Ok(format!("GPT {efi} + {root}"))
}

fn write_sfdisk(disk: &str, script: &str, progress: &mut ProgressState) -> Result<(), String> {
    // Write script to temp and sfdisk < file (works with elevate)
    let tmp = format!("/tmp/carve-sfdisk-{}.script", std::process::id());
    fs::write(&tmp, script).map_err(|e| e.to_string())?;
    progress.append_log(format!("partition: sfdisk script → {tmp}"));

    // sfdisk DISK < script — elevate may not redirect; use sh -c
    let cmd = format!("sfdisk --force {disk} < {tmp}");
    let r = run_root_ok("sh", &["-c", &cmd]);
    let _ = fs::remove_file(&tmp);
    r
}

fn wait_for_node(path: &str, tries: u32) -> Result<(), String> {
    for _ in 0..tries {
        if Path::new(path).exists() {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(100));
    }
    Err(format!("partition node never appeared: {path}"))
}

fn partition_loop_dry_run(
    plan: &mut InstallPlan,
    progress: &mut ProgressState,
) -> Result<String, String> {
    let img = plan.dry_run_root.join("disk.img");
    fs::create_dir_all(&plan.dry_run_root).map_err(|e| e.to_string())?;
    // 8 GiB sparse
    progress.append_log(format!("partition: loop dry-run image {}", img.display()));
    run_root_ok(
        "truncate",
        &["-s", "8G", img.to_str().unwrap()],
    )?;
    let loopdev = run_root_stdout("losetup", &["-f", "--show", img.to_str().unwrap()])?
        .trim()
        .to_string();
    if loopdev.is_empty() {
        return Err("losetup failed".into());
    }
    progress.append_log(format!("partition: loop device {loopdev}"));
    plan.disk = Some(DiskCandidate {
        path: loopdev.clone(),
        model: "loop dry-run".into(),
        size_bytes: 8 * 1024 * 1024 * 1024,
        is_live_medium: false,
    });
    // Temporarily treat as non-dry for partition ops on the loop
    let was = plan.dry_run;
    plan.dry_run = false;
    let r = job_partition(plan, progress);
    plan.dry_run = was;
    r.map(|s| format!("loop {loopdev}: {s}"))
}

pub fn job_format(plan: &mut InstallPlan, progress: &mut ProgressState) -> Result<String, String> {
    if plan.dry_run && plan.efi_part.as_deref() == Some("dry-run-efi") {
        progress.append_log("format: dry-run — skip mkfs");
        plan.root_uuid = std::env::var("ZAINIUM_ROOT_UUID").ok().filter(|s| !s.is_empty());
        return Ok("dry-run skip format".into());
    }

    let efi = plan
        .efi_part
        .as_deref()
        .ok_or("efi partition unknown")?;
    let root = plan
        .root_part
        .as_deref()
        .ok_or("root partition unknown")?;

    progress.append_log(format!("format: mkfs.vfat {efi}"));
    run_root_ok("mkfs.vfat", &["-F", "32", "-n", "ZAINIUMEFI", efi])?;

    progress.append_log(format!("format: mkfs.ext4 {root}"));
    run_root_ok("mkfs.ext4", &["-F", "-L", "zainium-root", root])?;

    // blkid UUID
    let uuid = blkid_uuid(root).or_else(|_| {
        // try via elevate
        run_root_stdout("blkid", &["-s", "UUID", "-o", "value", root])
            .map(|s| s.trim().to_string())
    })?;
    if uuid.is_empty() {
        return Err("empty root UUID after format".into());
    }
    plan.root_uuid = Some(uuid.clone());
    progress.append_log(format!("format: root UUID={uuid}"));
    Ok(format!("formatted efi+root uuid={uuid}"))
}

pub fn job_mount(plan: &mut InstallPlan, progress: &mut ProgressState) -> Result<String, String> {
    let root = target_root(plan);
    fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let efi_mp = root.join("efi");
    fs::create_dir_all(&efi_mp).map_err(|e| e.to_string())?;

    if plan.dry_run && plan.efi_part.as_deref() == Some("dry-run-efi") {
        progress.append_log(format!("mount: dry-run root {}", root.display()));
        return Ok(format!("dry-run root {}", root.display()));
    }

    let root_part = plan
        .root_part
        .as_deref()
        .ok_or("root partition unknown")?;
    let efi_part = plan.efi_part.as_deref().ok_or("efi partition unknown")?;

    progress.append_log(format!(
        "mount: {root_part} → {} ; {efi_part} → {}",
        root.display(),
        efi_mp.display()
    ));
    run_root_ok("mount", &[root_part, root.to_str().unwrap()])?;
    run_root_ok("mount", &[efi_part, efi_mp.to_str().unwrap()])?;
    Ok(format!("mounted {}", root.display()))
}

pub fn job_sync_unmount(
    plan: &mut InstallPlan,
    progress: &mut ProgressState,
) -> Result<String, String> {
    let root = target_root(plan);
    let efi_mp = root.join("efi");

    // Always write redacted plan snapshot
    let plan_path = root.join("INSTALL-PLAN.toml");
    let safe = InstallPlan {
        password: String::new(),
        ..plan.clone()
    };
    if let Ok(body) = toml::to_string_pretty(&safe) {
        let _ = fs::write(&plan_path, body);
        progress.append_log(format!("sync: wrote {}", plan_path.display()));
    }

    if plan.dry_run && plan.efi_part.as_deref() == Some("dry-run-efi") {
        progress.append_log("sync: dry-run complete");
        return Ok("dry-run synced".into());
    }

    progress.append_log("sync: sync + umount");
    let _ = run_root_ok("sync", &[]);
    // Unmount efi then root
    if efi_mp.exists() {
        let _ = run_root_ok("umount", &[efi_mp.to_str().unwrap()]);
    }
    let _ = run_root_ok("umount", &[root.to_str().unwrap()]);

    // Detach loop if used
    if let Some(disk) = &plan.disk {
        if disk.path.contains("loop") {
            let _ = run_root_ok("losetup", &["-d", &disk.path]);
        }
    }

    progress.append_log("sync: unmounted target");
    Ok("synced and unmounted".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partition_naming_scsi_and_nvme() {
        assert_eq!(partition_path("/dev/sda", 1), "/dev/sda1");
        assert_eq!(partition_path("/dev/sda", 2), "/dev/sda2");
        assert_eq!(partition_path("/dev/nvme0n1", 1), "/dev/nvme0n1p1");
        assert_eq!(partition_path("/dev/mmcblk0", 2), "/dev/mmcblk0p2");
        assert_eq!(partition_path("/dev/loop0", 1), "/dev/loop0p1");
    }
}
