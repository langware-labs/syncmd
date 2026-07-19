//! `syncmd` CLI — a thin wrapper over `syncmd_core::{plan, sync}`.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};
use owo_colors::{OwoColorize, Stream::Stdout};
use syncmd_core::report::GroupStatus;
use syncmd_core::{plan, sync, Error, Strategy, SyncOpts, SyncReport};

#[derive(Parser)]
#[command(
    name = "syncmd",
    about = "Keep equivalent AI-harness asset files converged on the latest change.",
    version
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Show what would change. Writes nothing.
    Plan {
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Comma-separated harness formats to include (default: all). See `syncmd formats`.
        #[arg(long, value_delimiter = ',')]
        formats: Option<Vec<String>>,
        #[arg(long)]
        json: bool,
        #[arg(long)]
        recursive: bool,
    },
    /// Converge the group(s) on the latest change.
    Sync {
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Comma-separated harness formats to include (default: all). See `syncmd formats`.
        #[arg(long, value_delimiter = ',')]
        formats: Option<Vec<String>>,
        /// How to resolve divergence (more than one member changed).
        #[arg(long, value_enum, default_value_t = StrategyArg::Newest)]
        strategy: StrategyArg,
        /// Compute the plan but write nothing.
        #[arg(long)]
        dry_run: bool,
        /// Back up overwritten members to <path>.syncmd.bak (default on).
        #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
        backup: bool,
        /// Create absent members from the winner (default on).
        #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
        create_missing: bool,
        /// Propagate a deletion across the group.
        #[arg(long)]
        allow_delete: bool,
        #[arg(long)]
        recursive: bool,
        #[arg(long)]
        json: bool,
    },
    /// List the known harness formats and where each one mounts.
    Formats,
}

#[derive(Copy, Clone, ValueEnum)]
enum StrategyArg {
    Newest,
    Error,
    Interactive,
}

impl From<StrategyArg> for Strategy {
    fn from(s: StrategyArg) -> Strategy {
        match s {
            StrategyArg::Newest => Strategy::Newest,
            StrategyArg::Error => Strategy::Error,
            StrategyArg::Interactive => Strategy::Interactive,
        }
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let (result, json) = match cli.cmd {
        Cmd::Plan {
            path,
            formats,
            json,
            recursive,
        } => {
            let opts = SyncOpts {
                formats,
                recursive,
                ..SyncOpts::default()
            };
            (plan(&path, &opts), json)
        }
        Cmd::Sync {
            path,
            formats,
            strategy,
            dry_run,
            backup,
            create_missing,
            allow_delete,
            recursive,
            json,
        } => {
            let opts = SyncOpts {
                formats,
                strategy: Some(strategy.into()),
                dry_run,
                backup,
                create_missing,
                allow_delete,
                recursive,
            };
            (sync(&path, &opts), json)
        }
        Cmd::Formats => {
            print_formats();
            return ExitCode::SUCCESS;
        }
    };

    match result {
        Ok(report) => {
            if json {
                println!("{}", serde_json::to_string_pretty(&report).unwrap());
            } else {
                print_human(&report);
            }
            ExitCode::from(report.exit_code() as u8)
        }
        Err(e) => {
            eprintln!("syncmd: {e}");
            ExitCode::from(exit_code(&e) as u8)
        }
    }
}

/// List every harness label with the mounts it maps to, per asset type.
/// Uses the repo's `syncmd.toml` when run inside one, else the built-in defaults.
fn print_formats() {
    let registry = syncmd_core::registry::Registry::load(std::path::Path::new("."))
        .unwrap_or_else(|_| syncmd_core::registry::Registry::builtin_defaults());
    for rule in &registry.rules {
        println!(
            "{} {}",
            rule.group.if_supports_color(Stdout, |t| t.bold()),
            format!("({})", rule.asset_type.as_str()).if_supports_color(Stdout, |t| t.dimmed()),
        );
        for m in &rule.mounts {
            println!(
                "    {} {}",
                format!("{:<14}", m.harness).if_supports_color(Stdout, |t| t.cyan()),
                m.pattern
            );
        }
    }
    println!(
        "\n{}",
        "Pass --formats=<a,b,c> to plan/sync to restrict to a subset (default: all)."
            .if_supports_color(Stdout, |t| t.dimmed())
    );
}

fn exit_code(e: &Error) -> i32 {
    e.exit_code()
}

fn print_human(report: &SyncReport) {
    if report.groups.is_empty() {
        println!(
            "{}",
            "No syncmd groups found under the given path."
                .if_supports_color(Stdout, |t| t.dimmed())
        );
        return;
    }
    for g in &report.groups {
        let status = colored_status(g.status);
        let winner = g
            .winner_path
            .as_deref()
            .map(|w| format!(" — winner: {w}"))
            .unwrap_or_default();
        println!(
            "{status} {} {}{}",
            g.name.if_supports_color(Stdout, |t| t.bold()),
            format!("({})", g.type_.as_str()).if_supports_color(Stdout, |t| t.dimmed()),
            winner.if_supports_color(Stdout, |t| t.dimmed()),
        );
        for m in &g.mounts {
            if m.action == "skip" {
                continue;
            }
            let action = colored_action(&m.action);
            let applied = if m.applied {
                String::new()
            } else {
                format!(
                    " {}",
                    "(planned)".if_supports_color(Stdout, |t| t.dimmed())
                )
            };
            println!("    {action} {}{applied}", m.path);
        }
        if !g.overridden.is_empty() {
            println!(
                "    {}",
                format!("overridden: {}", g.overridden.join(", "))
                    .if_supports_color(Stdout, |t| t.dimmed())
            );
        }
        if let Some(note) = &g.note {
            println!(
                "    {}",
                format!("note: {note}").if_supports_color(Stdout, |t| t.yellow())
            );
        }
    }
    print_summary(report);
}

fn colored_status(s: GroupStatus) -> String {
    match s {
        GroupStatus::InSync => format!("{}", "✓ in sync   ".if_supports_color(Stdout, |t| t.green())),
        GroupStatus::Propagated => {
            format!("{}", "↻ propagated".if_supports_color(Stdout, |t| t.cyan()))
        }
        GroupStatus::DivergedResolved => {
            format!("{}", "⇄ resolved  ".if_supports_color(Stdout, |t| t.yellow()))
        }
        GroupStatus::Conflict => {
            format!("{}", "✗ conflict  ".if_supports_color(Stdout, |t| t.style(owo_colors::style().red().bold())))
        }
        GroupStatus::Skipped => {
            format!("{}", "- skipped   ".if_supports_color(Stdout, |t| t.dimmed()))
        }
        GroupStatus::Noop => format!("{}", "· noop      ".if_supports_color(Stdout, |t| t.dimmed())),
    }
}

fn colored_action(action: &str) -> String {
    let padded = format!("{action:<7}");
    match action {
        "create" => format!("{}", padded.if_supports_color(Stdout, |t| t.green())),
        "write" | "update" => format!("{}", padded.if_supports_color(Stdout, |t| t.yellow())),
        "delete" => format!("{}", padded.if_supports_color(Stdout, |t| t.red())),
        _ => format!("{}", padded.if_supports_color(Stdout, |t| t.dimmed())),
    }
}

fn print_summary(report: &SyncReport) {
    let s = &report.summary;
    let conflicts = if s.conflicts > 0 {
        format!(
            "{}",
            format!("{} conflict", s.conflicts).if_supports_color(Stdout, |t| t.style(owo_colors::style().red().bold()))
        )
    } else {
        format!("{} conflict", s.conflicts)
    };
    let written = if s.written > 0 {
        format!(
            "{}",
            format!("{} file(s) written", s.written).if_supports_color(Stdout, |t| t.bold())
        )
    } else {
        format!("{} file(s) written", s.written)
    };
    println!(
        "\n{} group(s): {} in sync, {} propagated, {conflicts}, {} skipped, {written}.",
        s.groups, s.in_sync, s.propagated, s.skipped
    );
}
