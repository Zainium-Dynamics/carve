// Copyright (c) 2026 Zainiumdynamics. All rights reserved.
// Designed for Zainium OS by Zainiumdynamics — https://zainiumdynamics.tech

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum JobStatus {
    Pending,
    Running,
    Done,
    Failed,
    Skipped,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InstallJob {
    pub id: &'static str,
    pub label_key: &'static str,
    pub status: JobStatus,
    pub detail: String,
}

impl InstallJob {
    pub fn new(id: &'static str, label_key: &'static str) -> Self {
        Self {
            id,
            label_key,
            status: JobStatus::Pending,
            detail: String::new(),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct ProgressState {
    pub jobs: Vec<InstallJob>,
    pub log: String,
    pub fraction: f32,
    pub failed: bool,
    pub finished: bool,
}

impl ProgressState {
    pub fn pipeline() -> Self {
        let jobs = vec![
            InstallJob::new("preflight", "job-preflight"),
            InstallJob::new("partition", "job-partition"),
            InstallJob::new("format", "job-format"),
            InstallJob::new("mount", "job-mount"),
            InstallJob::new("seed", "job-seed"),
            InstallJob::new("deploy", "job-deploy"),
            InstallJob::new("user", "job-user"),
            InstallJob::new("quantra", "job-quantra"),
            InstallJob::new("eclipse", "job-eclipse"),
            InstallJob::new("sync", "job-sync"),
        ];
        Self {
            jobs,
            log: String::new(),
            fraction: 0.0,
            failed: false,
            finished: false,
        }
    }

    pub fn append_log(&mut self, line: impl AsRef<str>) {
        let line = line.as_ref();
        log::info!("{line}");
        if !self.log.is_empty() {
            self.log.push('\n');
        }
        self.log.push_str(line);
    }
}
