# carve

Zainium OS CLI installer. Plain terminal binary — no GUI, no Wayland, no
libcosmic/iced/wgpu. Works over a bare console or serial tty.

One pipeline (`InstallPlan` → `backend::run_install`), two frontends:

```bash
carve install            # interactive wizard, dry-run by default
carve install --real     # interactive wizard, real disk install
carve auto-install --config autoinstall.toml   # headless, no prompts
carve check-assets       # what's actually staged, right now
carve apply-install-conf --out limine.conf --root-uuid <uuid>   # test a limine.install.conf template
```

## Interactive wizard

Locale/keyboard → network (DHCP or Wi-Fi via `quantra-netd`, or skip) →
timezone → disk → user account → summary → install. Dry-run by default;
`--real` partitions for real and requires typing `ERASE` at the summary.

No root password step — Zainium has no `sudo`. The account goes into
`wheel` + `/etc/elevators/elevate.toml` for `elevate`. On a successful
real install, offers to reboot via `quantra-ctl shutdown --reboot`.

## Headless `auto-install`

What the `zainium.auto_install=1` boot entry runs. Every value comes from
the TOML config; missing or invalid values are rejected outright, not
guessed. (The boot-time wiring that launches it — `zainium.auto_install`
parsing in `quantra-ramfs`, and the actual exec hook — isn't part of this
crate.)

```toml
# autoinstall.toml
[system]
locale   = "en_US.UTF-8"   # default en_US.UTF-8
keyboard = "us"            # default "us"
timezone = "Asia/Karachi"  # default UTC — checked against /usr/share/zoneinfo
hostname = "zainium"       # default "zainium"

[user]
full_name    = "Ali Zain"
username     = "alizain"
password_env = "CARVE_PASSWORD"   # preferred over a plaintext `password =`

[disk]
select = "largest"   # "largest" (default) | "only" | an explicit "/dev/sdX"

[install]
dry_run      = false
reboot_after = false
```

```bash
CARVE_PASSWORD='...' carve auto-install --config /path/to/autoinstall.toml

# --config defaults to /overlayer/zaisys/carve/autoinstall.toml
CARVE_PASSWORD='...' carve auto-install
```

## Install log

Every run writes the full job log to `INSTALL-LOG.txt` at the root of the
target, success or failure — the only record of what happened on a
headless install with nobody watching the terminal.

## Timezones

`src/tzdata.rs` walks `/usr/share/zoneinfo` (or `ZAINIUM_ZONEINFO`) at
runtime for the region/city list and for validating a config-supplied
zone — whatever the medium's `tzdata` actually ships, nothing more.

## Network

`src/netclient.rs` talks to `quantra-netd` directly (length-prefixed JSON
over `/run/quantra-system/quantra-netd.sock`, same protocol as
`quantra-net`/`quantra-ctl`) for real DHCP/Wi-Fi. If the daemon isn't
reachable, the network step is skipped.

## Env vars

`ZAINIUM_ZAIROOT`, `ZAINIUM_OVERLAYER`, `ZAINIUM_ZAISYS`, `ZAINIUM_SQUASH`,
`ZAINIUM_LIMINE_LIVE_CONF`, `ZAINIUM_LIMINE_INSTALL_CONF`,
`ZAINIUM_ROOT_UUID`, `ZAINIUM_ROOT_FSTYPE`, `ZAINIUM_KERNEL`,
`ZAINIUM_INITRD`, `ZAINIUM_DRY_RUN_ROOT`, `ZAINIUM_QUANTRA_ENABLE`,
`ZAINIUM_ELEVATE`.

## v1 scope

- Disk layout: 2-partition ESP(FAT32)+ROOT(ext4), `syshub`/`zaisys`/
  `zexlib` merged via OverlayFS inside ROOT. A physical 3-partition split
  is a separate, larger change.
- Encryption (LUKS): not wired up yet.
- Bootloader: Limine only (`src/backend/limine.rs`).

## App path

`/overlayer/syshub/bin/carve`
