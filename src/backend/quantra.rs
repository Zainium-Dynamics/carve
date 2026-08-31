// Copyright (c) 2026 Zainiumdynamics. All rights reserved.
// Designed for Zainium OS by Zainiumdynamics — https://zainiumdynamics.tech
//
// Never systemctl. Never invent service TOMLs — only enable markers for services
// that already exist on the target (copied from LFS syshub).

use std::fs;

use crate::model::{InstallPlan, ProgressState};

use super::{
    config::InstallerConfig,
    paths::{quantra_enabled_dir, quantra_services_dir, syshub, target_root},
};

pub fn job_enable_services(
    plan: &InstallPlan,
    progress: &mut ProgressState,
) -> Result<String, String> {
    let cfg = InstallerConfig::from_env();
    let root = target_root(plan);
    let services = quantra_services_dir(&root);
    let enabled = quantra_enabled_dir(&root);

    // Hostname always writable under target syshub/etc when tree exists
    let etc = syshub(&root).join("etc");
    if etc.is_dir() || plan.dry_run {
        fs::create_dir_all(&etc).map_err(|e| e.to_string())?;
        fs::write(etc.join("hostname"), format!("{}\n", plan.hostname)).map_err(|e| e.to_string())?;
        progress.append_log(format!("quantra: hostname={}", plan.hostname));
        // Timezone: symlink-free, plain text — same "no assumptions" spirit
        // as hostname. Value already validated against the live
        // /usr/share/zoneinfo tree by the caller (wizard or autoinstall
        // config), never a hardcoded zone here.
        fs::write(etc.join("timezone"), format!("{}\n", plan.timezone)).map_err(|e| e.to_string())?;
        progress.append_log(format!("quantra: timezone={}", plan.timezone));
    }

    if !services.is_dir() {
        if plan.dry_run {
            progress.append_log(
                "quantra: dry-run — services/ not present yet (no fake TOMLs written)",
            );
            return Ok("quantra deferred (no service tree)".into());
        }
        return Err(format!(
            "quantra services dir missing: {} (deploy OS first)",
            services.display()
        ));
    }

    fs::create_dir_all(&enabled).map_err(|e| e.to_string())?;

    let mut enabled_n = 0usize;
    let mut skipped = Vec::new();
    for name in &cfg.quantra_enable {
        let toml = services.join(format!("{name}.toml"));
        if !toml.is_file() {
            skipped.push(name.clone());
            progress.append_log(format!(
                "quantra: skip enable {name} (no {}.toml on target)",
                name
            ));
            continue;
        }
        let marker = enabled.join(name);
        fs::write(&marker, b"").map_err(|e| e.to_string())?;
        enabled_n += 1;
        progress.append_log(format!("quantra: enabled {name}"));
    }

    if enabled_n == 0 && !plan.dry_run {
        return Err(format!(
            "no Quantra services enabled; missing TOMLs for: {}",
            cfg.quantra_enable.join(", ")
        ));
    }

    progress.append_log("quantra: markers only — never systemctl");
    Ok(format!(
        "enabled={enabled_n} skipped={}",
        skipped.len()
    ))
}
