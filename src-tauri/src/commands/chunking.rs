//! Splits the texture download into chunks that can be fetched and retried
//! one at a time.
//!
//! The plan is built from the partial clone's own tree objects (which arrive
//! with the clone; only file contents are deferred), so it needs no API calls
//! and adapts to any repository layout. Weight is the number of *unique*
//! blobs a chunk adds: texture packs reuse identical files heavily and git
//! downloads each blob once, so unique blobs track download size far better
//! than file counts.

use std::collections::{BTreeMap, HashMap, HashSet};

/// One `git sparse-checkout add` invocation.
#[derive(Debug, Clone, PartialEq)]
pub struct Chunk {
    /// Repository-relative directory paths to add.
    pub dirs: Vec<String>,
    /// Number of blobs this chunk is expected to download.
    pub blobs: usize,
}

/// Parse `git ls-tree -r -z HEAD -- <path>` output into (path, blob id) pairs.
/// Entries that are not blobs, or are not valid UTF-8, are skipped; the final
/// full `sparse-checkout set` downloads anything a plan misses.
pub fn parse_ls_tree(output: &[u8]) -> Vec<(String, String)> {
    output
        .split(|b| *b == 0)
        .filter(|entry| !entry.is_empty())
        .filter_map(|entry| {
            let entry = std::str::from_utf8(entry).ok()?;
            let (meta, path) = entry.split_once('\t')?;
            let mut fields = meta.split_whitespace();
            let _mode = fields.next()?;
            let kind = fields.next()?;
            let id = fields.next()?;
            (kind == "blob").then(|| (path.to_string(), id.to_string()))
        })
        .collect()
}

#[derive(Default)]
struct Node {
    children: BTreeMap<String, Node>,
    /// Unique blob ids anywhere under this directory.
    blobs: HashSet<u32>,
    /// Unique blob ids of files directly in this directory.
    direct: HashSet<u32>,
}

/// Plan chunks for everything under `root`.
///
/// A directory with more than `max_blobs` unique blobs is split into its
/// subdirectories; smaller directories are packed together, in path order,
/// until adding the next one would exceed `max_blobs` or `max_chars` (the
/// command-line budget for the directory arguments). Files sitting directly
/// in a split directory are fetched with its first subdirectory, because cone
/// mode includes the files of every parent of an included directory, so they
/// are counted towards that subdirectory's chunk.
///
/// A directory that cannot be split (no subdirectories) becomes a chunk on
/// its own regardless of size.
pub fn plan_chunks(
    root: &str,
    files: &[(String, String)],
    max_blobs: usize,
    max_chars: usize,
) -> Vec<Chunk> {
    let prefix = format!("{}/", root.trim_end_matches('/'));
    let mut ids: HashMap<&str, u32> = HashMap::new();
    let mut tree = Node::default();

    for (path, blob) in files {
        let Some(relative) = path.strip_prefix(&prefix) else {
            continue;
        };
        let next_id = ids.len() as u32;
        let id = *ids.entry(blob.as_str()).or_insert(next_id);

        let mut node = &mut tree;
        node.blobs.insert(id);
        let mut parts: Vec<&str> = relative.split('/').collect();
        parts.pop(); // file name
        for part in parts {
            node = node.children.entry(part.to_string()).or_default();
            node.blobs.insert(id);
        }
        node.direct.insert(id);
    }

    if tree.blobs.is_empty() {
        return Vec::new();
    }

    let mut units: Vec<(String, HashSet<u32>)> = Vec::new();
    collect_units(
        &tree,
        root.trim_end_matches('/').to_string(),
        max_blobs,
        &mut HashSet::new(),
        &mut units,
    );

    let mut chunks = Vec::new();
    let mut seen: HashSet<u32> = HashSet::new();
    let mut current = Chunk { dirs: Vec::new(), blobs: 0 };
    let mut current_chars = 0;

    for (dir, blobs) in units {
        let new_blobs = blobs.iter().filter(|id| !seen.contains(id)).count();
        let chars = dir.len() + 3; // quotes and separating space
        if !current.dirs.is_empty()
            && (current.blobs + new_blobs > max_blobs || current_chars + chars > max_chars)
        {
            chunks.push(std::mem::replace(&mut current, Chunk { dirs: Vec::new(), blobs: 0 }));
            current_chars = 0;
        }
        current.dirs.push(dir);
        current.blobs += new_blobs;
        current_chars += chars;
        seen.extend(blobs.iter().copied());
    }
    if !current.dirs.is_empty() {
        chunks.push(current);
    }

    chunks
}

/// `inherited` holds files directly in split ancestors that no unit has
/// claimed yet; the next unit emitted is the one cone mode fetches them with.
fn collect_units(
    node: &Node,
    path: String,
    max_blobs: usize,
    inherited: &mut HashSet<u32>,
    units: &mut Vec<(String, HashSet<u32>)>,
) {
    if node.blobs.len() <= max_blobs || node.children.is_empty() {
        let mut blobs = node.blobs.clone();
        blobs.extend(inherited.drain());
        units.push((path, blobs));
        return;
    }
    inherited.extend(node.direct.iter().copied());
    for (name, child) in &node.children {
        collect_units(child, format!("{}/{}", path, name), max_blobs, inherited, units);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(entries: &[(&str, &str)]) -> Vec<(String, String)> {
        entries
            .iter()
            .map(|(p, b)| (p.to_string(), b.to_string()))
            .collect()
    }

    #[test]
    fn parses_ls_tree_output() {
        let out = b"100644 blob aaa\ttex/a b/one.png\0100644 blob bbb\ttex/two&three.png\0160000 commit ccc\ttex/sub\0";
        assert_eq!(
            parse_ls_tree(out),
            files(&[("tex/a b/one.png", "aaa"), ("tex/two&three.png", "bbb")])
        );
    }

    #[test]
    fn small_root_is_a_single_chunk() {
        let f = files(&[("root/a/1.png", "1"), ("root/b/2.png", "2")]);
        let chunks = plan_chunks("root", &f, 10, 1000);
        assert_eq!(chunks, vec![Chunk { dirs: vec!["root".into()], blobs: 2 }]);
    }

    #[test]
    fn large_directories_are_split_and_small_ones_packed() {
        let f = files(&[
            ("root/big/x/1.png", "1"),
            ("root/big/x/2.png", "2"),
            ("root/big/y/3.png", "3"),
            ("root/big/y/4.png", "4"),
            ("root/small/5.png", "5"),
        ]);
        let chunks = plan_chunks("root", &f, 2, 1000);
        assert_eq!(
            chunks,
            vec![
                Chunk { dirs: vec!["root/big/x".into()], blobs: 2 },
                Chunk { dirs: vec!["root/big/y".into()], blobs: 2 },
                Chunk { dirs: vec!["root/small".into()], blobs: 1 },
            ]
        );
    }

    #[test]
    fn shared_blobs_count_once() {
        // Both teams use the same two textures; only the first download pays.
        let f = files(&[
            ("root/teams/a/1.png", "1"),
            ("root/teams/a/2.png", "2"),
            ("root/teams/b/1.png", "1"),
            ("root/teams/b/2.png", "2"),
            ("root/teams/c/3.png", "3"),
            ("root/teams/c/4.png", "4"),
        ]);
        let chunks = plan_chunks("root", &f, 2, 1000);
        assert_eq!(
            chunks,
            vec![
                Chunk { dirs: vec!["root/teams/a".into(), "root/teams/b".into()], blobs: 2 },
                Chunk { dirs: vec!["root/teams/c".into()], blobs: 2 },
            ]
        );
    }

    #[test]
    fn files_in_split_directories_count_towards_first_subdirectory() {
        // "root" and "root/big" are split; their own files are fetched with
        // the first unit beneath them, root/big/x.
        let f = files(&[
            ("root/top.png", "t"),
            ("root/big/loose.png", "l"),
            ("root/big/x/1.png", "1"),
            ("root/big/y/2.png", "2"),
            ("root/big/y/3.png", "3"),
            ("root/small/4.png", "4"),
        ]);
        let chunks = plan_chunks("root", &f, 3, 1000);
        assert_eq!(
            chunks,
            vec![
                Chunk { dirs: vec!["root/big/x".into()], blobs: 3 },
                Chunk { dirs: vec!["root/big/y".into(), "root/small".into()], blobs: 3 },
            ]
        );
    }

    #[test]
    fn unsplittable_directory_stays_whole() {
        let f = files(&[("root/flat/1.png", "1"), ("root/flat/2.png", "2"), ("root/flat/3.png", "3")]);
        let chunks = plan_chunks("root", &f, 1, 1000);
        assert_eq!(chunks, vec![Chunk { dirs: vec!["root/flat".into()], blobs: 3 }]);
    }

    #[test]
    fn respects_command_line_budget() {
        let f = files(&[("root/aaaa/1.png", "1"), ("root/bbbb/2.png", "2"), ("root/x/3.png", "3")]);
        // Each "root/aaaa" costs 12 chars; a budget of 20 fits one per chunk.
        let chunks = plan_chunks("root", &f, 2, 20);
        assert!(chunks.iter().all(|c| c.dirs.len() == 1), "{:?}", chunks);
    }

    #[test]
    fn ignores_files_outside_root_and_empty_input() {
        let f = files(&[("other/1.png", "1")]);
        assert!(plan_chunks("root", &f, 10, 1000).is_empty());
    }
}
