//! `anvil doctor` — inspect the dev environment and report which speedup
//! tools are installed, which are missing, and how to install the missing
//! ones. Estimates the time-savings each unlocks.

use anyhow::Result;
use std::process::Command;

pub fn run() -> Result<()> {
    println!("anvil doctor — dev-loop speed diagnostics\n");

    let mut checks: Vec<Check> = vec![
        Check::cmd(
            "rustc",
            "rust toolchain",
            "Rust compiler",
            "required",
            "https://rustup.rs",
        ),
        Check::cmd(
            "cargo",
            "cargo",
            "Rust package manager + build tool",
            "required",
            "https://rustup.rs",
        ),
        Check::cargo_subcommand(
            "watch",
            "auto-rebuild + restart on source edits — `anvil dev` uses this",
            "huge dev-loop win",
            "cargo install cargo-watch",
        ),
        Check::cargo_subcommand(
            "nextest",
            "faster `cargo test` runner with cleaner output",
            "~30% faster test runs",
            "cargo install cargo-nextest --locked",
        ),
        Check::cmd(
            "sccache",
            "sccache",
            "cross-project compiler-output cache — warm rebuilds are near-instant",
            "30-70% faster cold rebuilds",
            "cargo install sccache --locked",
        ),
        Check::cmd_optional(
            "mold",
            "mold linker",
            "fast Linux linker (3-10× faster linking)",
            "huge link-time win",
            "sudo apt install mold  # or build from source",
            cfg!(target_os = "linux"),
        ),
        Check::cmd_optional(
            "ld64.lld",
            "lld linker (LLVM)",
            "alternative fast linker for macOS",
            "moderate link-time win",
            "brew install llvm",
            cfg!(target_os = "macos"),
        ),
        Check::nightly_component(
            "rustc_codegen_cranelift",
            "Cranelift codegen backend (nightly Rust)",
            "2-3× faster rustc in dev (no LLVM optimizations)",
            r#"rustup toolchain install nightly
   rustup component add rustc-codegen-cranelift-preview --toolchain nightly"#,
        ),
    ];

    let mut installed = 0;
    let mut optional_missing = 0;
    let mut required_missing = 0;

    for check in &mut checks {
        check.run();
        if check.skipped {
            continue;
        }
        match (check.present, check.required) {
            (true, _) => {
                println!("  ✓  {}", check.name);
                if let Some(version) = &check.version {
                    println!("       {}", version);
                }
                installed += 1;
            }
            (false, true) => {
                println!("  ✗  {} (MISSING — required)", check.name);
                println!("       {}", check.purpose);
                println!("       install: {}", check.install);
                required_missing += 1;
            }
            (false, false) => {
                println!("  ○  {} (missing — {})", check.name, check.benefit);
                println!("       {}", check.purpose);
                println!("       install: {}", check.install);
                optional_missing += 1;
            }
        }
        println!();
    }

    println!("───────────────────────────────────────────────────────────────");
    println!(
        "  installed: {installed}    optional missing: {optional_missing}    required missing: {required_missing}"
    );
    if optional_missing > 0 {
        println!();
        println!("  Pick the wins you want and re-run `anvil doctor`.");
        println!("  Stacking sccache + cargo-watch + (mold/lld) + Cranelift = the");
        println!("  current shortest path from edit-save to running app.");
    } else if required_missing == 0 {
        println!();
        println!("  Everything's installed — your dev loop is fully tuned.");
    }
    Ok(())
}

struct Check {
    name: &'static str,
    purpose: &'static str,
    benefit: &'static str,
    install: String,
    required: bool,
    skipped: bool,
    present: bool,
    version: Option<String>,
    probe: ProbeKind,
}

enum ProbeKind {
    Cmd(&'static str),
    CargoSubcommand(&'static str),
    NightlyComponent(&'static str),
}

impl Check {
    fn cmd(
        cmd: &'static str,
        name: &'static str,
        purpose: &'static str,
        benefit: &'static str,
        install: &'static str,
    ) -> Self {
        Self {
            name,
            purpose,
            benefit,
            install: install.to_string(),
            required: benefit == "required",
            skipped: false,
            present: false,
            version: None,
            probe: ProbeKind::Cmd(cmd),
        }
    }

    fn cmd_optional(
        cmd: &'static str,
        name: &'static str,
        purpose: &'static str,
        benefit: &'static str,
        install: &'static str,
        applicable: bool,
    ) -> Self {
        Self {
            name,
            purpose,
            benefit,
            install: install.to_string(),
            required: false,
            skipped: !applicable,
            present: false,
            version: None,
            probe: ProbeKind::Cmd(cmd),
        }
    }

    fn cargo_subcommand(
        sub: &'static str,
        purpose: &'static str,
        benefit: &'static str,
        install: &'static str,
    ) -> Self {
        Self {
            name: match sub {
                "watch" => "cargo-watch",
                "nextest" => "cargo-nextest",
                _ => "cargo subcommand",
            },
            purpose,
            benefit,
            install: install.to_string(),
            required: false,
            skipped: false,
            present: false,
            version: None,
            probe: ProbeKind::CargoSubcommand(sub),
        }
    }

    fn nightly_component(
        component: &'static str,
        name: &'static str,
        benefit: &'static str,
        install: &'static str,
    ) -> Self {
        Self {
            name,
            purpose: "alternative codegen backend used during dev builds",
            benefit,
            install: install.to_string(),
            required: false,
            skipped: false,
            present: false,
            version: None,
            probe: ProbeKind::NightlyComponent(component),
        }
    }

    fn run(&mut self) {
        if self.skipped {
            return;
        }
        match self.probe {
            ProbeKind::Cmd(c) => {
                if let Ok(out) = Command::new(c).arg("--version").output() {
                    if out.status.success() {
                        self.present = true;
                        self.version = String::from_utf8_lossy(&out.stdout)
                            .lines()
                            .next()
                            .map(String::from);
                    }
                }
            }
            ProbeKind::CargoSubcommand(sub) => {
                let result = Command::new("cargo")
                    .args([sub, "--version"])
                    .stderr(std::process::Stdio::null())
                    .output();
                if let Ok(out) = result {
                    if out.status.success() {
                        self.present = true;
                        self.version = String::from_utf8_lossy(&out.stdout)
                            .lines()
                            .next()
                            .map(String::from);
                    }
                }
            }
            ProbeKind::NightlyComponent(comp) => {
                let out = Command::new("rustup")
                    .args(["component", "list", "--installed", "--toolchain", "nightly"])
                    .output();
                if let Ok(out) = out {
                    let stdout = String::from_utf8_lossy(&out.stdout);
                    if stdout.lines().any(|l| l.contains(comp)) {
                        self.present = true;
                        self.version = Some(format!("nightly: {comp}"));
                    }
                }
            }
        }
    }
}
