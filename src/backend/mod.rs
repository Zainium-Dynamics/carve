// Copyright (c) 2026 Zainiumdynamics. All rights reserved.
// Designed for Zainium OS by Zainiumdynamics — https://zainiumdynamics.tech
//
// Install backend: LFS · EROFS/SquashFS · Limine (your conf) · elevate (no sudo) · elevate-crypto.

pub mod assets;
pub mod config;
pub mod deploy;
pub mod disk;
pub mod elevate_cmd;
pub mod limine;
pub mod paths;
pub mod preflight;
pub mod quantra;
pub mod user;

use crate::model::{InstallPlan, JobStatus, ProgressState};

/// Run the full install pipeline. Plan is mutated (partitions, UUID).
pub fn run_install(
    plan: &mut InstallPlan,
    progress: &mut ProgressState,
    mut on_progress: impl FnMut(&ProgressState),
) -> Result<(), String> {
    let total = progress.jobs.len() as f32;
    for (i, job_id) in progress
        .jobs
        .iter()
        .map(|j| j.id)
        .collect::<Vec<_>>()
        .into_iter()
        .enumerate()
    {
        if let Some(job) = progress.jobs.iter_mut().find(|j| j.id == job_id) {
            job.status = JobStatus::Running;
        }
        progress.fraction = i as f32 / total;
        on_progress(progress);

        let result = match job_id {
            "preflight" => preflight::job_preflight(plan, progress),
            "partition" => disk::job_partition(plan, progress),
            "format" => disk::job_format(plan, progress),
            "mount" => disk::job_mount(plan, progress),
            "seed" => deploy::job_seed_tree(plan, progress),
            "deploy" => deploy::job_deploy_os(plan, progress),
            "user" => user::job_create_user(plan, progress),
            "quantra" => quantra::job_enable_services(plan, progress),
            "limine" => limine::job_install_boot(plan, progress),
            "sync" => disk::job_sync_unmount(plan, progress),
            other => Err(format!("unknown job {other}")),
        };

        match result {
            Ok(detail) => {
                if let Some(job) = progress.jobs.iter_mut().find(|j| j.id == job_id) {
                    job.status = JobStatus::Done;
                    job.detail = detail;
                }
            }
            Err(err) => {
                if let Some(job) = progress.jobs.iter_mut().find(|j| j.id == job_id) {
                    job.status = JobStatus::Failed;
                    job.detail = err.clone();
                }
                progress.failed = true;
                progress.append_log(format!("FAIL {job_id}: {err}"));
                on_progress(progress);
                write_install_log(plan, progress);
                return Err(err);
            }
        }
        progress.fraction = (i + 1) as f32 / total;
        on_progress(progress);
    }

    progress.finished = true;
    progress.fraction = 1.0;
    progress.append_log("Install finished successfully");
    on_progress(progress);
    write_install_log(plan, progress);
    Ok(())
}

/// Best-effort — a log-write failure never masks or overrides the real
/// install result. Written to the target root itself so a failure on a
/// headless auto-install (nobody watching the terminal) is still
/// debuggable after the fact.
fn write_install_log(plan: &InstallPlan, progress: &ProgressState) {
    let path = paths::target_root(plan).join("INSTALL-LOG.txt");
    if let Err(e) = std::fs::write(&path, &progress.log) {
        eprintln!(
            "carve: warning — could not write install log to {}: {e}",
            path.display()
        );
    }
}

/// Reboot through `quantra-ctl` when present (graceful service stop via
/// PID 1's control socket — never `systemctl` on this OS); falls back to
/// a bare `reboot` syscall via `elevate` otherwise.
pub fn reboot() -> Result<(), String> {
    if elevate_cmd::which("quantra-ctl") {
        elevate_cmd::run_root_ok("quantra-ctl", &["shutdown", "--reboot"])
    } else {
        elevate_cmd::run_root_ok("reboot", &[])
    }
}
