// Copyright (c) 2026 Zainiumdynamics. All rights reserved.
// Designed for Zainium OS by Zainiumdynamics — https://zainiumdynamics.tech
//
// Kept GUI-free on purpose — shared as-is between the wizard and the
// headless auto-install path in this same crate.

pub mod install_plan;
pub mod progress;

pub use install_plan::{DiskCandidate, InstallMode, InstallPlan, PartitionMode};
pub use progress::{JobStatus, ProgressState};
