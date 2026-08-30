use clap::{Parser, Subcommand};
use engine::assets::asset_paths::AssetEntry;

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
    /// Show every mount serving an asset name, resolution winner first
    Which {
        /// A lookup key: mount-relative path, bare filename where the family
        /// collapses basenames, or a namespace-qualified name like "iface/log.pcx"
        name: String,
    },
}

/// The families the tool enumerates, in the game's lookup priority order:
/// every family the game consults, plus the raw data files (gamesys, missions,
/// motiondb) as a pseudo-family, plus `fonts` on a classic install (mounted
/// from `res/fonts.crf` outside the family list there).
fn family_names() -> Vec<&'static str> {
    let mut names: Vec<&'static str> = shock2vr::resource_families().to_vec();
    if !shock2vr::is_25th_anniversary_install() {
        names.push("fonts");
    }
    names.push("data");
    names
}

fn family_entries(family: &str) -> Vec<AssetEntry> {
    if family == "data" {
        let mounts = engine::assets::asset_paths::AssetPath::combine(
            shock2vr::data_files::data_file_mounts(shock2vr::paths::data_root()),
        );
        // The data mount spans all of `data/`, which contains the res/
        // families already listed as their own families; keep only the
        // root-level files (gamesys, missions, motiondb).
        return mounts
            .entries()
            .into_iter()
            .filter(|entry| !entry.key.contains('/'))
            .collect();
    }
    shock2vr::resource_family_paths(family).entries()
}

/// Mounts the game consults that this tool cannot enumerate, so results can be
/// incomplete: a classic install's loose `res/mesh` / `res/obj` folder mounts
/// (which outrank the archives) and its loose data-root files.
fn print_coverage_caveat() {
    if !shock2vr::is_25th_anniversary_install() {
        eprintln!(
            "note: classic install - loose res/mesh and res/obj folders (which outrank the \
             archives) and loose data-root files are not enumerated; results may be incomplete"
        );
    }
}

/// Archive path relative to the data root, for compact display.
fn short_source(source: &str) -> String {
    let root = shock2vr::paths::data_root().to_string_lossy().into_owned();
    source
        .strip_prefix(&format!("{root}/"))
        .unwrap_or(source)
        .to_string()
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
        Commands::Which { name } => which(name),
    }
}
