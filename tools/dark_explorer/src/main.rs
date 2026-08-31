use clap::{Parser, Subcommand};
use engine::assets::asset_paths::AssetEntry;

mod explorer;
mod model_preview;
mod ui;

use explorer::{family_entries, family_names, print_coverage_caveat, short_source};

#[derive(Parser)]
#[command(name = "dark_explorer")]
#[command(about = "Explore the mounted game archives (KPF/CRF) without unzipping them")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// List mounted archives per resource family (or one family's assets)
    Ls {
        /// Resource family to list assets for (e.g. obj, snd, mesh); omit for an overview
        family: Option<String>,

        /// Only show keys containing this substring (case-insensitive)
        #[arg(long)]
        filter: Option<String>,

        /// Maximum assets to print
        #[arg(long, default_value_t = 50)]
        limit: usize,
    },
    /// Search all families for assets whose key contains a substring
    Find {
        /// Case-insensitive substring to match against asset keys
        pattern: String,

        /// Maximum matches to print
        #[arg(long, default_value_t = 50)]
        limit: usize,
    },
    /// Open a windowed asset browser (tree + search + preview)
    Ui {
        /// Write a PNG of the first rendered frame to this path and exit
        #[arg(long)]
        screenshot: Option<std::path::PathBuf>,

        /// Open with an asset selected, as "<family>/<key>" (e.g. "obj/txt16/arm.pcx")
        #[arg(long)]
        select: Option<String>,

        /// Open with the search box pre-filled with this filter
        #[arg(long)]
        search: Option<String>,

        /// Start the 3D model preview with the skeleton overlay on
        #[arg(long)]
        skeletons: bool,

        /// Start the 3D model preview with the hitbox overlay on
        #[arg(long)]
        hitboxes: bool,
    },
    /// Show every mount serving an asset name, resolution winner first
    Which {
        /// A lookup key: mount-relative path, bare filename where the family
        /// collapses basenames, or a namespace-qualified name like "iface/log.pcx"
        name: String,
    },
}

fn ls(family: Option<String>, filter: Option<String>, limit: usize) {
    print_coverage_caveat();
    match family {
        None => {
            if filter.is_some() {
                eprintln!("--filter needs a family (e.g. ls obj --filter door)");
                std::process::exit(1);
            }
            // Overview: per family, per archive, how many files each serves.
            for family in family_names() {
                let mut per_source: Vec<(String, usize)> = Vec::new();
                for entry in family_entries(family) {
                    if entry.is_alias {
                        continue;
                    }
                    match per_source.iter_mut().find(|(s, _)| *s == entry.source) {
                        Some((_, count)) => *count += 1,
                        None => per_source.push((entry.source, 1)),
                    }
                }
                if per_source.is_empty() {
                    continue;
                }
                let total: usize = per_source.iter().map(|(_, count)| count).sum();
                println!("{family} ({total} assets)");
                for (source, count) in per_source {
                    println!("  {} ({count})", short_source(&source));
                }
            }
        }
        Some(family) => {
            let known = family_names();
            if !known.contains(&family.as_str()) {
                eprintln!("unknown family '{family}' (known: {})", known.join(", "));
                std::process::exit(1);
            }
            let needle = filter.map(|f| f.to_ascii_lowercase());
            let mut entries: Vec<AssetEntry> = family_entries(&family)
                .into_iter()
                .filter(|e| !e.is_alias)
                .filter(|e| needle.as_ref().is_none_or(|n| e.key.contains(n)))
                .collect();
            entries.sort_by(|a, b| a.key.cmp(&b.key));
            for entry in entries.iter().take(limit) {
                println!("{}\t{}", entry.key, short_source(&entry.source));
            }
            if entries.len() > limit {
                println!("... {} more (raise --limit)", entries.len() - limit);
            }
        }
    }
}

fn find(pattern: String, limit: usize) {
    print_coverage_caveat();
    let needle = pattern.to_ascii_lowercase();
    let mut shown = 0;
    let mut total = 0;
    for family in family_names() {
        for entry in family_entries(family) {
            if entry.is_alias || !entry.key.contains(&needle) {
                continue;
            }
            total += 1;
            if shown < limit {
                println!("{family}\t{}\t{}", entry.key, short_source(&entry.source));
                shown += 1;
            }
        }
    }
    if total > shown {
        println!("... {} more (raise --limit)", total - shown);
    }
    if total == 0 {
        println!("no assets matching '{pattern}'");
    }
}

fn which(name: String) {
    print_coverage_caveat();
    let query = name.to_ascii_lowercase();
    // The game resolves one combined mount list (families in this same order),
    // so exactly one match can win; everything after it is shadowed. Aliases
    // count - they are real lookup keys - but only where the mount actually
    // registered one (`strings` collapses no basenames, for example).
    let mut winner_marked = false;
    for family in family_names() {
        for entry in family_entries(family) {
            if entry.key != query {
                continue;
            }
            let marker = if winner_marked { "shadowed" } else { "WINNER" };
            winner_marked = true;
            println!(
                "{family}\t{marker}\t{}\t{} ({})",
                entry.key,
                short_source(&entry.source),
                entry.entry_name
            );
        }
    }
    if !winner_marked {
        println!("no mount serves '{name}'");
        std::process::exit(1);
    }
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Commands::Ls {
            family,
            filter,
            limit,
        } => ls(family, filter, limit),
        Commands::Find { pattern, limit } => find(pattern, limit),
        Commands::Ui {
            screenshot,
            select,
            search,
            skeletons,
            hitboxes,
        } => ui::run(ui::UiOptions {
            screenshot,
            select,
            search,
            skeletons,
            hitboxes,
        }),
        Commands::Which { name } => which(name),
    }
}
