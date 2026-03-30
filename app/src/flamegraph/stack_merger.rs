use roaring::RoaringBitmap;
use rustc_hash::FxHashMap;
use std::fs::File;
use std::io::{self, BufRead, BufReader};

/// A simple string interner that maps unique strings to compact `u32` IDs.
/// Identical strings share the same ID, reducing allocations in the trie.
#[derive(Default)]
struct StringInterner {
    map: FxHashMap<String, u32>,
    strings: Vec<String>,
}

impl StringInterner {
    /// Return the ID for `s`, inserting it if not already present.
    fn intern(&mut self, s: &str) -> u32 {
        if let Some(&id) = self.map.get(s) {
            return id;
        }
        let id = self.strings.len() as u32;
        let owned = s.to_string();
        self.map.insert(owned.clone(), id);
        self.strings.push(owned);
        id
    }

    /// Look up the string for a given ID.
    #[inline]
    fn get(&self, id: u32) -> &str {
        &self.strings[id as usize]
    }
}

/// Represents a node in the Trie structure for stack traces.
/// Children are keyed by interned string IDs for memory efficiency.
#[derive(Debug)]
pub struct TrieNode {
    children: FxHashMap<u32, TrieNode>,
    is_end_of_stack: bool,
    ranks: RoaringBitmap,
}

impl TrieNode {
    fn new() -> Self {
        TrieNode {
            children: FxHashMap::default(),
            is_end_of_stack: false,
            ranks: RoaringBitmap::new(),
        }
    }

    #[inline]
    fn add_rank(&mut self, rank: u32) {
        self.ranks.insert(rank);
    }
}

/// Represents a Trie structure for merging stack traces.
pub struct StackTrie {
    pub root: TrieNode,
    all_ranks: RoaringBitmap,
    interner: StringInterner,
}

impl StackTrie {
    fn new(all_ranks: Vec<u32>) -> Self {
        let all_ranks_bitmap: RoaringBitmap = all_ranks.into_iter().collect();
        StackTrie {
            root: TrieNode::new(),
            all_ranks: all_ranks_bitmap,
            interner: StringInterner::default(),
        }
    }

    /// Create a new StackTrie with known total rank count.
    /// Use this when processing ranks in batches but need consistent rank formatting.
    pub fn with_total_ranks(total_ranks: u32) -> Self {
        let all_ranks_bitmap: RoaringBitmap = (0..total_ranks).collect();
        StackTrie {
            root: TrieNode::new(),
            all_ranks: all_ranks_bitmap,
            interner: StringInterner::default(),
        }
    }

    /// Insert a batch of stacks with their rank IDs.
    /// Can be called multiple times to incrementally build the trie.
    ///
    /// # Arguments
    /// * `stacks` - Vector of (rank_id, folded_stack_string) pairs
    pub fn insert_batch(&mut self, stacks: Vec<(u32, &str)>) {
        for (rank, stack) in stacks {
            let stack_frames: Vec<&str> = stack.split(';').collect();
            self.insert(stack_frames, rank);
        }
    }

    fn insert(&mut self, stack: Vec<&str>, rank: u32) {
        // Skip empty stacks
        if stack.is_empty() {
            return;
        }

        let mut node = &mut self.root;
        for frame in stack {
            // Skip empty frame names
            if frame.is_empty() {
                continue;
            }
            let id = self.interner.intern(frame);
            node = node
                .children
                .entry(id)
                .or_insert_with(TrieNode::new);
            node.add_rank(rank);
        }
        node.is_end_of_stack = true;
        node.add_rank(rank);
    }

    fn format_rank_str(&self, ranks: &RoaringBitmap) -> String {
        let leak_ranks: RoaringBitmap = &self.all_ranks - ranks;

        fn inner_format(bitmap: &RoaringBitmap) -> String {
            if bitmap.is_empty() {
                return String::new();
            }
            // Estimate capacity: ~6 chars per element (e.g. "12345/") as a reasonable average.
            // Ranges like "0-9999" use fewer slots; single values like "12345" use more.
            let mut buf = String::with_capacity(bitmap.len() as usize * 6);
            let mut iter = bitmap.iter().peekable();
            let mut first_segment = true;
            while let Some(start) = iter.next() {
                let mut end = start;
                // Extend run as long as values are consecutive
                while iter.peek() == Some(&(end + 1)) {
                    end = iter.next().unwrap();
                }
                if !first_segment {
                    buf.push('/');
                }
                first_segment = false;
                if start == end {
                    buf.push_str(&start.to_string());
                } else {
                    buf.push_str(&format!("{}-{}", start, end));
                }
            }
            buf
        }

        let has_stack_ranks = inner_format(ranks);
        let leak_stack_ranks = inner_format(&leak_ranks);
        format!("@{}|{}", has_stack_ranks, leak_stack_ranks)
    }

    pub fn traverse_with_all_stack<'a>(
        &'a self,
        node: &'a TrieNode,
        path: Vec<&str>,
    ) -> Vec<(Vec<String>, String)> {
        let mut result = Vec::new();
        for (&id, child) in &node.children {
            let frame = self.interner.get(id);
            let rank_str = self.format_rank_str(&child.ranks);
            if child.is_end_of_stack {
                let path_str = path.join(";");
                result.push((vec![path_str, frame.to_string()], rank_str.clone()));
            }
            let mut child_path = path.clone();
            let frame_rank = format!("{}{}", frame, rank_str);
            child_path.push(&frame_rank[..]);
            result.extend(self.traverse_with_all_stack(child, child_path));
        }
        result
    }
}

/// Merges multiple stack traces into a single StackTrie.
pub fn merge_stacks(stacks: Vec<&str>) -> StackTrie {
    let all_ranks: Vec<u32> = (0..stacks.len() as u32).collect();
    let mut trie = StackTrie::new(all_ranks);
    for (rank, stack) in stacks.iter().enumerate() {
        let stack_frames: Vec<&str> = stack.split(';').collect();
        trie.insert(stack_frames, rank as u32);
    }
    trie
}

#[allow(dead_code)]
fn read_file_to_list(file_path: &str) -> io::Result<Vec<String>> {
    let file = File::open(file_path)?;
    let reader = BufReader::new(file);
    let mut lines = Vec::new();
    for line in reader.lines() {
        let line = line?;
        lines.push(line);
    }
    Ok(lines)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_stack_handling() {
        let stacks = vec!["", "main;func1", ""];
        let trie = merge_stacks(stacks);
        // Empty stacks should be skipped, so we should only have 1 stack
        let results = trie.traverse_with_all_stack(&trie.root, Vec::new());
        assert!(
            !results.is_empty(),
            "Should have at least one result for non-empty stack"
        );
    }

    #[test]
    fn test_empty_frame_handling() {
        let stacks = vec!["main;;func1", "main;func2"];
        let trie = merge_stacks(stacks);
        let results = trie.traverse_with_all_stack(&trie.root, Vec::new());
        // Should handle empty frames gracefully
        assert!(
            !results.is_empty(),
            "Should process stacks despite empty frames"
        );
    }

    #[test]
    fn test_merge_stacks_basic() {
        let stacks = vec!["main;func1;func2", "main;func1;func3"];
        let trie = merge_stacks(stacks);
        let results = trie.traverse_with_all_stack(&trie.root, Vec::new());
        assert_eq!(results.len(), 2, "Should have 2 distinct paths");
    }

    #[test]
    fn test_with_total_ranks_creates_correct_ranks() {
        let trie = StackTrie::with_total_ranks(5);
        assert_eq!(trie.all_ranks.len(), 5);
        assert!(trie.all_ranks.contains(0));
        assert!(trie.all_ranks.contains(4));
        assert!(!trie.all_ranks.contains(5));
    }

    #[test]
    fn test_insert_batch_multiple_calls() {
        let mut trie = StackTrie::with_total_ranks(4);

        // First batch: ranks 0 and 1
        trie.insert_batch(vec![(0, "main;func1;func2"), (1, "main;func1;func3")]);

        // Second batch: ranks 2 and 3
        trie.insert_batch(vec![(2, "main;func1;func2"), (3, "main;func2;func4")]);

        let results = trie.traverse_with_all_stack(&trie.root, Vec::new());
        // Should have 3 distinct paths: func2 (ranks 0,2), func3 (rank 1), func4 (rank 3)
        assert_eq!(results.len(), 3, "Should have 3 distinct paths");
    }

    #[test]
    fn test_incremental_vs_all_at_once_consistency() {
        // Process all at once using merge_stacks
        let stacks_all = vec![
            "main;func1;func2",
            "main;func1;func3",
            "main;func1;func2",
            "main;func2;func4",
        ];
        let trie_all = merge_stacks(stacks_all);
        let results_all = trie_all.traverse_with_all_stack(&trie_all.root, Vec::new());

        // Process incrementally using with_total_ranks + insert_batch
        let mut trie_incremental = StackTrie::with_total_ranks(4);
        trie_incremental.insert_batch(vec![(0, "main;func1;func2"), (1, "main;func1;func3")]);
        trie_incremental.insert_batch(vec![(2, "main;func1;func2"), (3, "main;func2;func4")]);
        let results_incremental =
            trie_incremental.traverse_with_all_stack(&trie_incremental.root, Vec::new());

        // Both should produce the same number of paths
        assert_eq!(
            results_all.len(),
            results_incremental.len(),
            "Incremental and all-at-once should produce same number of paths"
        );

        // Convert results to comparable format (sort for deterministic comparison)
        let mut paths_all: Vec<_> = results_all
            .iter()
            .map(|(p, r)| (p.clone(), r.clone()))
            .collect();
        let mut paths_incremental: Vec<_> = results_incremental
            .iter()
            .map(|(p, r)| (p.clone(), r.clone()))
            .collect();
        paths_all.sort();
        paths_incremental.sort();

        assert_eq!(
            paths_all, paths_incremental,
            "Incremental and all-at-once should produce identical results"
        );
    }
}

//////////////////////////////////////////////////////////////////////////

// let stacks = vec![
//     "main;func1;func2;func3",
//     "main;func1;func2;func4",
//     "main;func1;func3;func5",
//     "main;func1;func3;func6",
// ];

// let trie = merge_stacks(stacks);

// let mut output = File::create("./output/merged_stacks.txt")?;
// for (path, rank_str) in trie.traverse_with_all_stack(&trie.root, Vec::new()) {
//     writeln!(output, "{} {} 1", path.join(";"), rank_str)?;
// }

////////////////////////////////////////////////////////////////////////////////

