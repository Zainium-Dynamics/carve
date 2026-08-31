// Copyright (c) 2026 Zainiumdynamics. All rights reserved.
// Designed for Zainium OS by Zainiumdynamics — https://zainiumdynamics.tech
//
// Interactive step-by-step install wizard. One full banner at Welcome,
// then a compact breadcrumb per step (no repeated banner).

use std::io::{self, Write as _};

use inquire::{Confirm, Password, PasswordDisplayMode, Select, Text};

use crate::{
    backend,
    model::{install_plan::valid_username, DiskCandidate, InstallPlan, JobStatus, ProgressState},
    netclient, tzdata,
};

// Welcome, Locale&Keyboard, Network, Timezone, Disk, User, Summary,
// Installing, Finished — one `screen.header()` call per step.
const TOTAL_STEPS: usize = 9;

struct Screen {
    step: usize,
    banner_shown: bool,
}

impl Screen {
    fn new() -> Self {
        Self {
            step: 0,
            banner_shown: false,
        }
    }

    fn header(&mut self, title: &str) {
        self.step += 1;
        clear_screen();
        if !self.banner_shown {
            println!("╔══════════════════════════════════════════╗");
            println!("║           Z A I N I U M   O S             ║");
            println!("║              carve · installer            ║");
            println!("╚══════════════════════════════════════════╝");
            println!();
            self.banner_shown = true;
        }
        println!("carve · Step {}/{TOTAL_STEPS} · {title}", self.step);
        println!("{}", "─".repeat(46));
        println!();
    }
}

fn clear_screen() {
    print!("\x1B[2J\x1B[H");
    let _ = io::stdout().flush();
}

/// Run the interactive wizard. `real` selects a genuine disk install;
/// otherwise the whole pipeline runs under `dry_run` (writes under
/// `dry_run_root`, never touches a real block device) — same safe-by-
/// default posture the GUI installer ships with.
pub fn run(real: bool) -> i32 {
    let mut plan = InstallPlan::default();
    plan.dry_run = !real;

    let mut screen = Screen::new();

    step_welcome(&mut screen, &plan);

    let Some(()) = step_locale_keyboard(&mut screen, &mut plan) else {
        return cancelled();
    };

    step_network(&mut screen);

    let Some(()) = step_timezone(&mut screen, &mut plan) else {
        return cancelled();
    };

    let Some(disk) = step_disk(&mut screen, &plan) else {
        return cancelled();
    };
    plan.disk = Some(disk);

    let Some(()) = step_user(&mut screen, &mut plan) else {
        return cancelled();
    };

    if !step_summary_confirm(&mut screen, &plan) {
        println!("\nCancelled — nothing was touched.\n");
        return 1;
    }

    step_progress_and_run(&mut screen, &mut plan)
}

fn cancelled() -> i32 {
    println!("\nCancelled — nothing was touched.\n");
    130
}

fn step_welcome(screen: &mut Screen, plan: &InstallPlan) {
    screen.header("Welcome");
    println!("This installs Zainium OS onto a disk on this machine.");
    if plan.dry_run {
        println!("\nMode: DRY-RUN — nothing on any real disk will be touched.");
        println!("(pass `carve install --real` to perform a genuine install)");
    } else {
        println!("\nMode: REAL DISK INSTALL — this will erase the target disk.");
    }
    pause();
}

fn step_locale_keyboard(screen: &mut Screen, plan: &mut InstallPlan) -> Option<()> {
    screen.header("Locale & Keyboard");
    plan.locale = Text::new("Locale:")
        .with_default(&plan.locale)
        .prompt()
        .ok()?;
    plan.keyboard = Text::new("Keyboard layout:")
        .with_default(&plan.keyboard)
        .prompt()
        .ok()?;
    Some(())
}

fn step_network(screen: &mut Screen) {
    screen.header("Network");

    if !netclient::is_available() {
        println!("quantra-netd isn't reachable from here — skipping network setup.");
        println!("(you can still install offline; connect later from the desktop)");
        pause();
        return;
    }

    let options = vec![
        "Wired — DHCP".to_string(),
        "Wireless (Wi-Fi)".to_string(),
        "Skip".to_string(),
    ];
    let Ok(choice) = Select::new("Network setup:", options).prompt() else {
        return;
    };

    match choice.as_str() {
        "Wired — DHCP" => network_wired(),
        "Wireless (Wi-Fi)" => network_wifi(),
        _ => println!("Skipped."),
    }
    pause();
}

fn network_wired() {
    let ifaces = match netclient::list_interfaces() {
        Ok(v) => v,
        Err(e) => {
            println!("Could not list interfaces: {e}");
            return;
        }
    };
    let names: Vec<String> = ifaces
        .iter()
        .filter(|i| i.name != "lo")
        .map(|i| i.name.clone())
        .collect();
    if names.is_empty() {
        println!("No interfaces found.");
        return;
    }
    let Ok(iface) = Select::new("Interface:", names).prompt() else {
        return;
    };
    println!("Acquiring DHCP lease on {iface}…");
    match netclient::dhcp_acquire(&iface) {
        Ok(lease) => {
            println!(
                "✓ Connected — {} via {}",
                lease.ip_cidr.as_deref().unwrap_or("?"),
                lease.gateway.as_deref().unwrap_or("?")
            );
        }
        Err(e) => println!("DHCP failed: {e}"),
    }
}

fn network_wifi() {
    let ifaces = match netclient::list_interfaces() {
        Ok(v) => v,
        Err(e) => {
            println!("Could not list interfaces: {e}");
            return;
        }
    };
    let names: Vec<String> = ifaces.iter().map(|i| i.name.clone()).collect();
    if names.is_empty() {
        println!("No interfaces found.");
        return;
    }
    let Ok(iface) = Select::new("Wireless interface:", names).prompt() else {
        return;
    };

    println!("Scanning for networks on {iface}…");
    let nets = match netclient::wifi_scan(&iface) {
        Ok(v) => v,
        Err(e) => {
            println!("Scan failed: {e}");
            return;
        }
    };
    if nets.is_empty() {
        println!("No networks found.");
        return;
    }
    let labels: Vec<String> = nets
        .iter()
        .map(|n| format!("{}  ({:?}, signal {})", n.ssid, n.security, n.signal))
        .collect();
    let Ok(pick) = Select::new("Network:", labels.clone()).prompt() else {
        return;
    };
    let idx = labels.iter().position(|l| l == &pick).unwrap_or(0);
    let net = &nets[idx];

    let password = if net.security == quantra_net_common::WifiSecurity::Open {
        None
    } else {
        Password::new("Wi-Fi password:")
            .with_display_mode(PasswordDisplayMode::Masked)
            .without_confirmation()
            .prompt()
            .ok()
    };

    println!("Connecting to {}…", net.ssid);
    match netclient::wifi_connect(&iface, &net.ssid, password, net.security.clone()) {
        Ok(()) => {
            println!("✓ Connected to {}", net.ssid);
            if let Ok(lease) = netclient::dhcp_acquire(&iface) {
                println!(
                    "  {} via {}",
                    lease.ip_cidr.as_deref().unwrap_or("?"),
                    lease.gateway.as_deref().unwrap_or("?")
                );
            }
        }
        Err(e) => println!("Connect failed: {e}"),
    }
}

fn step_timezone(screen: &mut Screen, plan: &mut InstallPlan) -> Option<()> {
    screen.header("Timezone");

    if !tzdata::available() {
        println!("No tzdata found on this medium — defaulting to UTC.");
        plan.timezone = "UTC".to_string();
        pause();
        return Some(());
    }

    let mut regions = tzdata::regions();
    for standalone in tzdata::standalone_zones() {
        regions.push(format!("(standalone) {standalone}"));
    }
    let region = Select::new("Region:", regions).prompt().ok()?;

    if let Some(zone) = region.strip_prefix("(standalone) ") {
        plan.timezone = zone.to_string();
    } else {
        let cities = tzdata::zones_in_region(&region);
        let city = Select::new("City:", cities).prompt().ok()?;
        plan.timezone = format!("{region}/{city}");
    }

    println!("\nTimezone: {}", plan.timezone);
    pause();
    Some(())
}

fn step_disk(screen: &mut Screen, plan: &InstallPlan) -> Option<DiskCandidate> {
    screen.header("Target Disk");

    let disks = backend::disk::list_disks();
    let candidates: Vec<&DiskCandidate> = disks.iter().filter(|d| !d.is_live_medium).collect();

    if candidates.is_empty() {
        println!("No usable disks found (only the live medium itself, or none at all).");
        pause();
        return None;
    }

    if !plan.dry_run {
        println!("⚠  Whole-disk install — the selected disk will be WIPED.\n");
    }

    let labels: Vec<String> = candidates.iter().map(|d| d.label()).collect();
    let pick = Select::new("Disk:", labels.clone()).prompt().ok()?;
    let idx = labels.iter().position(|l| l == &pick)?;
    Some(candidates[idx].clone())
}

fn step_user(screen: &mut Screen, plan: &mut InstallPlan) -> Option<()> {
    screen.header("User Account");

    plan.full_name = Text::new("Full name:").prompt().ok()?;

    loop {
        let username = Text::new("Username:").prompt().ok()?;
        if !valid_username(&username) {
            println!("Invalid username — lowercase, start with a letter, [a-z0-9_-] only.");
            continue;
        }
        plan.username = username;
        break;
    }

    loop {
        let pw1 = Password::new("Password:")
            .with_display_mode(PasswordDisplayMode::Masked)
            .without_confirmation()
            .prompt()
            .ok()?;
        if pw1.len() < 4 {
            println!("Too short — at least 4 characters.");
            continue;
        }
        let pw2 = Password::new("Confirm password:")
            .with_display_mode(PasswordDisplayMode::Masked)
            .without_confirmation()
            .prompt()
            .ok()?;
        if pw1 != pw2 {
            println!("Passwords didn't match — try again.");
            continue;
        }
        plan.password = pw1;
        break;
    }

    plan.hostname = Text::new("Hostname:")
        .with_default(&plan.hostname)
        .prompt()
        .ok()?;

    Some(())
}

fn step_summary_confirm(screen: &mut Screen, plan: &InstallPlan) -> bool {
    screen.header("Summary");

    println!("  Locale       {}", plan.locale);
    println!("  Keyboard     {}", plan.keyboard);
    println!("  Timezone     {}", plan.timezone);
    println!(
        "  Disk         {}",
        plan.disk.as_ref().map(|d| d.label()).unwrap_or_default()
    );
    println!("  Full name    {}", plan.full_name);
    println!("  Username     {}", plan.username);
    println!("  Hostname     {}", plan.hostname);
    println!(
        "  Mode         {}",
        if plan.dry_run { "DRY-RUN" } else { "REAL DISK — WILL ERASE" }
    );
    println!();

    if plan.dry_run {
        Confirm::new("Proceed?")
            .with_default(true)
            .prompt()
            .unwrap_or(false)
    } else {
        let typed = Text::new("Type ERASE to confirm wiping the disk above:")
            .prompt()
            .unwrap_or_default();
        typed.trim() == "ERASE"
    }
}

fn step_progress_and_run(screen: &mut Screen, plan: &mut InstallPlan) -> i32 {
    screen.header("Installing");

    let mut progress = ProgressState::pipeline();
    let result = backend::run_install(plan, &mut progress, |p| {
        if let Some(job) = p.jobs.iter().rev().find(|j| j.status != JobStatus::Pending) {
            let mark = match job.status {
                JobStatus::Running => "…",
                JobStatus::Done => "✓",
                JobStatus::Failed => "✗",
                JobStatus::Skipped => "–",
                JobStatus::Pending => " ",
            };
            println!("  {mark} {:<10} {}", job.id, job.detail);
        }
    });

    println!();
    match result {
        Ok(()) => {
            screen.header("Finished");
            println!("✓ Install finished successfully.");
            if plan.dry_run {
                println!("  (dry-run — nothing was written to a real disk: {})", plan.dry_run_root.display());
            } else {
                let reboot_now = Confirm::new("Reboot now?")
                    .with_default(true)
                    .prompt()
                    .unwrap_or(false);
                if reboot_now {
                    println!("Rebooting…");
                    if let Err(e) = backend::reboot() {
                        println!("Reboot failed: {e} — remove the install media and reboot manually.");
                    }
                } else {
                    println!("  Remove the install media and reboot when ready.");
                }
            }
            0
        }
        Err(e) => {
            println!("✗ Install failed: {e}\n");
            println!("── log ──────────────────────────────────");
            println!("{}", progress.log);
            1
        }
    }
}

fn pause() {
    println!();
    let _ = Text::new("Press Enter to continue…").prompt();
}
