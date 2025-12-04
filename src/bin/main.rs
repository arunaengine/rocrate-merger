//! RO-Crate Consolidation CLI
//!
//! Command-line tool for consolidating RO-Crate hierarchies and merging crates.

use std::fs;
use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};
use serde_json::Value;

use rocrate_consolidate::{
    consolidate, load_from_url, parse_graph, to_json_string, ConsolidateError, ConsolidateInput,
    ConsolidateOptions, MergeCrate, NoOpLoader, SubcrateLoader, UrlLoader,
};

#[derive(Parser)]
#[command(name = "rocrate-consolidate")]
#[command(about = "Consolidate RO-Crate hierarchies into a single metadata file")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Consolidate a crate and its nested subcrates
    Consolidate(ConsolidateArgs),
    /// Merge multiple independent crates
    Merge(MergeArgs),
}

#[derive(Args)]
struct ConsolidateArgs {
    /// Path to RO-Crate directory, ro-crate-metadata.json file, or URL
    source: String,

    /// Output file (default: stdout)
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Pretty-print JSON output
    #[arg(long)]
    pretty: bool,

    /// Don't add Subcrate type to converted folders
    #[arg(long)]
    no_subcrate_type: bool,

    /// Don't extend @context with consolidation vocabulary
    #[arg(long)]
    no_extend_context: bool,
}

#[derive(Args)]
struct MergeArgs {
    /// Path or URL to main RO-Crate (will be the root)
    main: String,

    /// Crates to merge: --merge <path_or_url> --as <folder_id> [--name <name>]
    /// Can be repeated for multiple crates
    #[arg(long = "merge", value_name = "PATH_OR_URL")]
    merge_sources: Vec<String>,

    /// Folder IDs for merged crates (must match number of --merge args)
    #[arg(long = "as", value_name = "FOLDER_ID")]
    folder_ids: Vec<String>,

    /// Optional names for merged crate folders
    #[arg(long = "name", value_name = "NAME")]
    names: Vec<String>,

    /// Output file (default: stdout)
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Pretty-print JSON output
    #[arg(long)]
    pretty: bool,

    /// Don't add Subcrate type to converted folders
    #[arg(long)]
    no_subcrate_type: bool,

    /// Don't extend @context
    #[arg(long)]
    no_extend_context: bool,
}

/// Check if a source string is a URL
fn is_url(source: &str) -> bool {
    source.starts_with("http://") || source.starts_with("https://")
}

/// Filesystem-based subcrate loader
struct FilesystemLoader {
    base_path: PathBuf,
}

impl FilesystemLoader {
    fn new(base_path: PathBuf) -> Self {
        Self { base_path }
    }
}

impl SubcrateLoader for FilesystemLoader {
    fn load(
        &self,
        subcrate_id: &str,
        parent_namespace: &str,
        _subcrate_entity: Option<&Value>,
    ) -> Result<Vec<Value>, ConsolidateError> {
        // Build the path to the subcrate
        let subcrate_path = if parent_namespace.is_empty() {
            // Direct child of root
            let relative = subcrate_id.trim_start_matches("./").trim_end_matches('/');
            self.base_path.join(relative)
        } else {
            // Nested subcrate
            let full_path = format!(
                "{}/{}",
                parent_namespace,
                subcrate_id.trim_start_matches("./").trim_end_matches('/')
            );
            self.base_path.join(full_path)
        };

        // Load the metadata file
        let metadata_path = find_metadata_file(&subcrate_path)?;
        let content =
            fs::read_to_string(&metadata_path).map_err(|e| ConsolidateError::LoadError {
                path: metadata_path.display().to_string(),
                reason: e.to_string(),
            })?;

        parse_graph(&content, &metadata_path.display().to_string())
    }
}

/// Find ro-crate-metadata.json in a directory
fn find_metadata_file(dir: &PathBuf) -> Result<PathBuf, ConsolidateError> {
    let standard = dir.join("ro-crate-metadata.json");
    if standard.exists() {
        return Ok(standard);
    }

    // Look for *-ro-crate-metadata.json
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                if name.ends_with("-ro-crate-metadata.json") {
                    return Ok(entry.path());
                }
            }
        }
    }

    Err(ConsolidateError::LoadError {
        path: dir.display().to_string(),
        reason: "No ro-crate-metadata.json found".to_string(),
    })
}

/// Load a crate's @graph from a path (local file/directory)
fn load_graph_from_path(path: &PathBuf) -> Result<Vec<Value>, ConsolidateError> {
    let metadata_path = if path.is_dir() {
        find_metadata_file(path)?
    } else if path.is_file() {
        path.clone()
    } else {
        return Err(ConsolidateError::InvalidPath(path.clone()));
    };

    let content = fs::read_to_string(&metadata_path).map_err(|e| ConsolidateError::LoadError {
        path: metadata_path.display().to_string(),
        reason: e.to_string(),
    })?;

    parse_graph(&content, &metadata_path.display().to_string())
}

/// Load a crate's @graph from a URL
fn load_graph_from_url(url: &str) -> Result<Vec<Value>, ConsolidateError> {
    let (_, content) = load_from_url(url)?;
    parse_graph(&content, url)
}

/// Load a crate's @graph from either a URL or local path
fn load_graph(source: &str) -> Result<Vec<Value>, ConsolidateError> {
    if is_url(source) {
        load_graph_from_url(source)
    } else {
        load_graph_from_path(&PathBuf::from(source))
    }
}

/// Write output to file or stdout
fn write_output(content: &str, output: Option<&PathBuf>) -> Result<(), ConsolidateError> {
    match output {
        Some(path) => {
            fs::write(path, content)?;
            eprintln!("Wrote consolidated crate to {}", path.display());
        }
        None => {
            println!("{}", content);
        }
    }
    Ok(())
}

fn run_consolidate(args: ConsolidateArgs) -> Result<(), ConsolidateError> {
    let graph = load_graph(&args.source)?;

    let options = ConsolidateOptions {
        add_subcrate_type: !args.no_subcrate_type,
        extend_context: !args.no_extend_context,
    };

    // Choose loader based on source type
    let loader: Box<dyn SubcrateLoader> = if is_url(&args.source) {
        eprintln!("Loading from URL: {}", args.source);
        Box::new(UrlLoader::from_metadata_url(&args.source))
    } else {
        let path = PathBuf::from(&args.source);
        let base_path = if path.is_dir() {
            path
        } else {
            path.parent().map(|p| p.to_path_buf()).unwrap_or_default()
        };
        Box::new(FilesystemLoader::new(base_path))
    };

    let result = consolidate(ConsolidateInput::Single(graph), loader.as_ref(), &options)?;

    eprintln!(
        "Consolidated {} crates, {} total entities ({} merged)",
        result.stats.crates_consolidated, result.stats.total_entities, result.stats.merged_entities
    );

    let output = to_json_string(&result, args.pretty)?;
    write_output(&output, args.output.as_ref())
}

fn run_merge(args: MergeArgs) -> Result<(), ConsolidateError> {
    // Validate arguments
    if args.merge_sources.len() != args.folder_ids.len() {
        return Err(ConsolidateError::InvalidStructure(format!(
            "Number of --merge ({}) must match number of --as ({})",
            args.merge_sources.len(),
            args.folder_ids.len()
        )));
    }

    // Load main crate
    let main_graph = load_graph(&args.main)?;

    // Load crates to merge
    let mut others = Vec::new();
    for (i, (source, folder_id)) in args.merge_sources.iter().zip(&args.folder_ids).enumerate() {
        let graph = load_graph(source)?;
        let name = args.names.get(i).cloned();
        others.push(MergeCrate {
            graph,
            folder_id: folder_id.clone(),
            name,
        });
    }

    let options = ConsolidateOptions {
        add_subcrate_type: !args.no_subcrate_type,
        extend_context: !args.no_extend_context,
    };

    // Use NoOpLoader since we're explicitly merging
    let result = consolidate(
        ConsolidateInput::Merge {
            main: main_graph,
            others,
        },
        &NoOpLoader,
        &options,
    )?;

    eprintln!(
        "Merged {} crates, {} total entities ({} shared entities merged)",
        result.stats.crates_consolidated, result.stats.total_entities, result.stats.merged_entities
    );

    let output = to_json_string(&result, args.pretty)?;
    write_output(&output, args.output.as_ref())
}

fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Consolidate(args) => run_consolidate(args),
        Commands::Merge(args) => run_merge(args),
    };

    if let Err(e) = result {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}
