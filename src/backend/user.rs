// Copyright (c) 2026 Zainiumdynamics. All rights reserved.
// Designed for Zainium OS by Zainiumdynamics — https://zainiumdynamics.tech
//
// Accounts: passwd/shadow/group under syshub/etc.
// Password hash: **elevate-crypto** (Argon2id) — no OpenSSL, no classic PAM.
// Privilege: user in **wheel** + `/etc/elevators/elevate.toml` for **elevate**
// (Zainium has no sudo — no separate root password step, ever).

use std::{fs, io::Write, path::Path};

use crate::model::{InstallPlan, ProgressState};

use super::paths::{syshub, target_root};

const WHEEL_GID: u32 = 10;
const USER_UID: u32 = 1000;
const USER_GID: u32 = 1000;

pub fn job_create_user(plan: &InstallPlan, progress: &mut ProgressState) -> Result<String, String> {
    plan.validate_user()
        .map_err(|_| "invalid username/password".to_string())?;

    let root = target_root(plan);
    let home = root.join("home").join(&plan.username);
    fs::create_dir_all(&home).map_err(|e| e.to_string())?;

    // Prefer syshub/etc (LFS overlayer); also ensure /etc if tree has top-level etc
    let etc_syshub = syshub(&root).join("etc");
    fs::create_dir_all(&etc_syshub).map_err(|e| e.to_string())?;

    let hash = hash_password_elevate(&plan.password)?;
    progress.append_log("user: password hashed with elevate-crypto (Argon2id)");

    let passwd_line = format!(
        "{}:x:{USER_UID}:{USER_GID}:{}:/home/{}:/bin/bash\n",
        plan.username, plan.full_name, plan.username
    );
    let shadow_line = format!("{}:{hash}:20000:0:99999:7:::\n", plan.username);
    let group_line = format!("{}:x:{USER_GID}:\n", plan.username);
    let wheel_line = format!("wheel:x:{WHEEL_GID}:{}\n", plan.username);

    ensure_base_passwd(&etc_syshub)?;
    append_unique_line(&etc_syshub.join("passwd"), &plan.username, &passwd_line)?;
    append_unique_line(&etc_syshub.join("shadow"), &plan.username, &shadow_line)?;
    append_unique_line(&etc_syshub.join("group"), &plan.username, &group_line)?;
    ensure_wheel_group(&etc_syshub, &plan.username, &wheel_line)?;

    // Mirror into top-level /etc if present (some boots use non-overlay etc)
    let etc_root = root.join("etc");
    if etc_root.is_dir() || plan.dry_run {
        fs::create_dir_all(&etc_root).map_err(|e| e.to_string())?;
        ensure_base_passwd(&etc_root)?;
        append_unique_line(&etc_root.join("passwd"), &plan.username, &passwd_line)?;
        append_unique_line(&etc_root.join("shadow"), &plan.username, &shadow_line)?;
        append_unique_line(&etc_root.join("group"), &plan.username, &group_line)?;
        ensure_wheel_group(&etc_root, &plan.username, &wheel_line)?;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(etc_syshub.join("shadow"), fs::Permissions::from_mode(0o400));
        if etc_root.join("shadow").exists() {
            let _ = fs::set_permissions(etc_root.join("shadow"), fs::Permissions::from_mode(0o400));
        }
        // home ownership best-effort (may need root on real install)
        let _ = fs::set_permissions(&home, fs::Permissions::from_mode(0o755));
    }

    // elevate policy (not sudoers) — classic grant syntax, file name elevate.toml
    write_elevators_policy(&etc_syshub, progress)?;
    if etc_root.exists() {
        write_elevators_policy(&etc_root, progress)?;
    }

    // elevate-pam service stacks (TOML only — never pam.d)
    seed_elevate_pam_services(&etc_syshub, progress)?;
    if etc_root.exists() {
        seed_elevate_pam_services(&etc_root, progress)?;
    }

    progress.append_log(format!(
        "user: {} uid={USER_UID} home={} groups=wheel (elevate-pam auth, no pam.d)",
        plan.username,
        home.display()
    ));
    Ok(format!("user {}", plan.username))
}

/// Hash with elevate-crypto Argon2id (preferred on Zainium / elevate-pam).
fn hash_password_elevate(password: &str) -> Result<String, String> {
    elevate_crypto::hash_password(password).map_err(|e| format!("elevate-crypto hash: {e}"))
}

fn ensure_base_passwd(etc: &Path) -> Result<(), String> {
    let passwd = etc.join("passwd");
    if !passwd.exists() {
        fs::write(
            &passwd,
            "root:x:0:0:root:/root:/bin/bash\n",
        )
        .map_err(|e| e.to_string())?;
    }
    let group = etc.join("group");
    if !group.exists() {
        fs::write(&group, "root:x:0:\n").map_err(|e| e.to_string())?;
    }
    let shadow = etc.join("shadow");
    if !shadow.exists() {
        fs::write(&shadow, "root:!:20000:0:99999:7:::\n").map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn ensure_wheel_group(etc: &Path, username: &str, wheel_line: &str) -> Result<(), String> {
    let group = etc.join("group");
    let existing = fs::read_to_string(&group).unwrap_or_default();
    if let Some(line) = existing.lines().find(|l| l.starts_with("wheel:")) {
        // ensure user is in member list
        if line.contains(username) {
            return Ok(());
        }
        let mut out = String::new();
        for l in existing.lines() {
            if l.starts_with("wheel:") {
                if l.ends_with(':') {
                    out.push_str(&format!("wheel:x:{WHEEL_GID}:{username}\n"));
                } else {
                    out.push_str(l);
                    out.push(',');
                    out.push_str(username);
                    out.push('\n');
                }
            } else {
                out.push_str(l);
                out.push('\n');
            }
        }
        fs::write(&group, out).map_err(|e| e.to_string())?;
    } else {
        append_unique_line(&group, "wheel", wheel_line)?;
    }
    Ok(())
}

fn write_elevators_policy(etc: &Path, progress: &mut ProgressState) -> Result<(), String> {
    let dir = etc.join("elevators");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join("elevate.toml");
    // Classic elevate/sudoers-style grant for wheel (elevate binary reads this path).
    // See zex-native/elevate: /etc/elevators/elevate.toml — not Linux sudo.
    let body = "\
# Zainium elevate policy — written by carve
# NO sudo. Privilege tool is /bin/elevate (elevate-sudo + elevate-pam).
# Syntax: classic grant lines (elevate policy file).

Defaults env_reset
Defaults secure_path=\"/overlayer/syshub/bin:/overlayer/syshub/sbin:/bin:/sbin\"

root ALL=(ALL:ALL) ALL
%wheel ALL=(ALL:ALL) ALL
";
    if path.exists() {
        // Merge: ensure wheel line exists
        let cur = fs::read_to_string(&path).unwrap_or_default();
        if !cur.contains("%wheel") {
            let mut f = fs::OpenOptions::new()
                .append(true)
                .open(&path)
                .map_err(|e| e.to_string())?;
            writeln!(f, "\n%wheel ALL=(ALL:ALL) ALL").map_err(|e| e.to_string())?;
            progress.append_log(format!("user: appended %wheel to {}", path.display()));
        } else {
            progress.append_log(format!("user: elevate policy already has wheel ({})", path.display()));
        }
    } else {
        fs::write(&path, body).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o440));
        }
        progress.append_log(format!("user: wrote elevate policy {}", path.display()));
    }
    Ok(())
}

/// Seed minimal elevate-pam stacks under etc/elevate-pam/services (no pam.d).
fn seed_elevate_pam_services(etc: &Path, progress: &mut ProgressState) -> Result<(), String> {
    let dir = etc.join("elevate-pam/services");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let stacks: &[(&str, &str)] = &[
        (
            "elevate.toml",
            r#"[service]
name = "elevate"
description = "elevate privilege auth (no pam.d)"

[[auth]]
control = "required"
module = "unix"

[[account]]
control = "required"
module = "unix"

[[password]]
control = "required"
module = "unix"

[[session]]
control = "required"
module = "unix"
"#,
        ),
        (
            "login.toml",
            r#"[service]
name = "login"
description = "console/greeter login auth (no pam.d)"

[[auth]]
control = "required"
module = "unix"

[[account]]
control = "required"
module = "unix"

[[password]]
control = "required"
module = "unix"

[[session]]
control = "required"
module = "unix"
"#,
        ),
        (
            "cosmic-greeter.toml",
            r#"[service]
name = "cosmic-greeter"
description = "COSMIC greeter auth (no pam.d)"

[[auth]]
control = "required"
module = "unix"

[[account]]
control = "required"
module = "unix"

[[password]]
control = "required"
module = "unix"

[[session]]
control = "required"
module = "unix"
"#,
        ),
    ];
    for (name, body) in stacks {
        let p = dir.join(name);
        if !p.exists() {
            fs::write(&p, body).map_err(|e| e.to_string())?;
            progress.append_log(format!("user: elevate-pam stack {}", p.display()));
        }
    }
    // Explicitly refuse pam.d creation
    let pamd = etc.join("pam.d");
    if pamd.exists() {
        progress.append_log(format!(
            "user: note — {} exists but is unused (elevate-pam only)",
            pamd.display()
        ));
    }
    Ok(())
}

fn append_unique_line(path: &Path, key: &str, line: &str) -> Result<(), String> {
    let existing = fs::read_to_string(path).unwrap_or_default();
    let prefix = format!("{key}:");
    if existing.lines().any(|l| l.starts_with(&prefix)) {
        let mut out = String::new();
        for l in existing.lines() {
            if l.starts_with(&prefix) {
                out.push_str(line);
            } else {
                out.push_str(l);
                out.push('\n');
            }
        }
        fs::write(path, out).map_err(|e| e.to_string())?;
        return Ok(());
    }
    let mut f = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    f.write_all(line.as_bytes()).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn argon2_hash_roundtrip() {
        let h = hash_password_elevate("test-password-123").expect("hash");
        assert!(h.starts_with("$argon2"), "got {h}");
        assert!(elevate_crypto::verify_password("test-password-123", &h).unwrap());
        assert!(!elevate_crypto::verify_password("wrong", &h).unwrap());
    }
}
