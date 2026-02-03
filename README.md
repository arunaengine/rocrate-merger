# RO-Crate Merger

`rocrate-merger` is a Rust-based CLI and library designed to 
consolidate [RO-Crate](https://www.researchobject.org/ro-crate/) hierarchies into a single metadata file or merge 
independent RO-Crates.

## Features

- **Recursive Consolidation**: Automatically discovers and merges nested subcrates.
- **Multiple Sources**: Supports loading RO-Crates from:
    - Local directories
    - ZIP archives
    - Remote URLs (HTTP/HTTPS)
- **Provenance Tracking**: Adds `Subcrate` types and `consolidatedEntities` properties to track the origin of merged data.
- **CLI & Library**: Provides both a powerful command-line interface and a flexible Rust API.

## CLI Usage

```bash
git clone https://github.com/your-repo/rocrate-merger.git
cd rocrate-merger
cargo build --release
```
The binary will be available at `target/release/rocrate-consolidate`.

The tool provides two main subcommands: `consolidate` and `merge`.

### Consolidate

Consolidate a crate and all its nested subcrates into a single metadata file.

```bash
# Consolidate a local crate
rocrate-consolidate consolidate ./path/to/crate -o consolidated.json

# Consolidate from a URL with pretty-printed output
rocrate-consolidate consolidate https://example.org/crate --pretty
```

### Merge

Merge multiple independent crates into a main root crate, placing each under a specific folder.

```bash
rocrate-consolidate merge ./main-crate \
  --merge ./sub-crate-1 --as folder1 --name "First Sub-crate" \
  --merge https://example.org/crate2 --as folder2 \
  -o merged.json
```

## Library Usage

Add this to your `Cargo.toml`:

```toml
[dependencies]
rocrate-consolidate = { git = "https://github.com/arunaengine/rocrate-merger" }
```

### Example: Consolidating a Crate

```rust
use rocrate_consolidate::{
  ConsolidateInput, ConsolidateOptions, UrlLoader, consolidate, load_from_url, parse_graph,
};

fn main() {
  // Load your crate's @graph as Vec<serde_json::Value>
  let rocrate_url = "https://rocrate.s3.computational.bio.uni-giessen.de/ro-crate-metadata.json";
  let (_, content) = load_from_url(&rocrate_url).unwrap();
  let graph = parse_graph(&content, &rocrate_url).unwrap();

  let result = consolidate(
    ConsolidateInput::Single(graph),
    &UrlLoader::from_metadata_url(&rocrate_url),
    &ConsolidateOptions::default(),
  )
  .unwrap();

  println!(
    "Total of {} entities ({} merged) after consolidation of {} RO-Crates.",
    result.stats.total_entities, result.stats.merged_entities, result.stats.crates_consolidated
  );
}
```

## Vocabulary Extensions

To track consolidation, the following terms are used (prefixed with `https://purl.org/rocrate/consolidate#`):

- **`Subcrate`**: A type added to `Dataset` entities that were originally the root of a separate RO-Crate.
- **`consolidatedEntities`**: A property on a `Subcrate` entity that lists all entity IDs that originated from that specific crate.

## License

The API is licensed under either of

* Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
* MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option. Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion for Aruna by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.

## Contributing

If you have any ideas, suggestions, or issues, please don't hesitate to open an issue and/or PR. Contributions to this project are always welcome! We appreciate your help in making this project better.
