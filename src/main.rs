// Copyright (c) 2026 Zainiumdynamics. All rights reserved.
// Designed for Zainium OS by Zainiumdynamics — https://zainiumdynamics.tech
//
// carve — Zainium OS CLI installer. Plain terminal binary, no GUI/Wayland
// deps: works on a bare console or serial tty.
//
//   carve install [--real]        interactive step-by-step wizard
//   carve auto-install --config … headless, TOML-driven (no prompts)
//   carve check-assets            honest report of what's on disk right now
//
// App path: /overlayer/syshub/bin/carve

mod autoinstall;
mod backend;
mod model;
mod netclient;
mod tzdata;
mod wizard;

use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name = "carve", about = "Zainium OS CLI installer", version)]
struct Cli {
    #[command(subcommand)]
    cmd: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Interactive step-by-step install wizard (default if no subcommand given)
    Install {
        /// Perform a genuine disk install. Without this flag, runs under
        /// dry-run (writes under a scratch dir, never touches a real disk).
        #[arg(long)]
        real: bool,
    },
    /// Headless, non-interactive install driven by a TOML config — what
    /// the `zainium.auto_install=1` boot path runs. No prompts.
    #[command(name = "auto-install")]
    AutoInstall {
        #[arg(long, default_value_os_t = autoinstall::default_config_path())]
        config: PathBuf,
    },
    /// Discover kernel / Limine / confs / packed image / overlayer (honest report)
    #[command(name = "check-assets")]
    CheckAssets {
        #[arg(long)]
        write: Option<PathBuf>,
    },
    /// Apply YOUR limine.install.conf → out (only substitutes ${ROOT_UUID} / ${ROOT_FSTYPE})
    #[command(name = "apply-install-conf")]
    ApplyInstallConf {
        /// Path to your prepared install conf (default: discover under zaisys)
        #[arg(long)]
        src: Option<PathBuf>,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        root_uuid: String,
        #[arg(long)]
        root_fstype: Option<String>,
    },
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let cli = Cli::parse();
    let code = match cli.cmd.unwrap_or(Commands::Install { real: false }) {
        Commands::Install { real } => wizard::run(real),
        Commands::AutoInstall { config } => autoinstall::run(&config),
        Commands::CheckAssets { write } => cmd_check_assets(write),
        Commands::ApplyInstallConf {
            src,
            out,
            root_uuid,
            root_fstype,
        } => cmd_apply_install_conf(src, out, root_uuid, root_fstype),
    };
    std::process::exit(code);
}

fn cmd_check_assets(write: Option<PathBuf>) -> i32 {
    let cfg = backend::config::InstallerConfig::from_env();
    let report = backend::assets::discover(&cfg);
    print!("{}", report.to_toml_report());
    if let Some(path) = write {
        if let Err(e) = backend::assets::write_report(&path, &report) {
            eprintln!("write failed: {e}");
            return 1;
        }
        eprintln!("wrote {}", path.display());
    }
    0
}

fn cmd_apply_install_conf(
    src: Option<PathBuf>,
    out: PathBuf,
    root_uuid: String,
    root_fstype: Option<String>,
) -> i32 {
    let cfg = backend::config::InstallerConfig::from_env();
    let assets = backend::assets::discover(&cfg);
    let src = match src.or(assets.limine_install_conf) {
        Some(p) => p,
        None => {
            eprintln!(
                "no limine.install.conf — pass --src or place under overlayer/zaisys/limine/limine.install.conf"
            );
            return 1;
        }
    };
    let fstype = root_fstype.unwrap_or(cfg.root_fstype);
    match backend::limine::apply_install_conf_file(&src, &out, &root_uuid, &fstype) {
        Ok(()) => {
            eprintln!("applied {} → {}", src.display(), out.display());
            0
        }
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}
