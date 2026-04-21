//! 核心分叉检测算法
//!
//! 遍历 StackTrie 的分叉点，找到覆盖率低于阈值的少数派分支中的 rank

use std::collections::HashMap;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use roaring::RoaringBitmap;

use super::config::RankAnalysisConfig;
use super::types::{AnalysisTrigger, DivergencePoint, ProblematicRank, RankAnalysisResult};
use crate::flamegraph::stack_merger::{StackTrie, TrieNode};

/// 分析 StackTrie 中的分叉点，识别问题 rank
///
/// # 算法
/// 1. 从 root 开始递归遍历 Trie
/// 2. 在每个有多个 children 的节点处，检查各分支的 rank 覆盖率
/// 3. 覆盖率 < minority_threshold 的分支中的 rank 被标记为少数派
/// 4. 累计每个 rank 的 anomaly_score（出现在少数派分支的次数）
/// 5. 返回 anomaly_score > 0 的 rank 列表
pub fn analyze_trie(
    trie: &StackTrie,
    config: &RankAnalysisConfig,
    trigger: AnalysisTrigger,
) -> RankAnalysisResult {
    let start = Instant::now();
    let total_ranks = trie.all_ranks.len() as u32;

    if total_ranks == 0 {
        return RankAnalysisResult {
            total_ranks: 0,
            trigger,
            timestamp: now_secs(),
            minority_threshold: config.minority_threshold,
            ..Default::default()
        };
    }

    // rank_id -> (anomaly_score, divergence_points)
    let mut rank_scores: HashMap<u32, (u32, Vec<DivergencePoint>)> = HashMap::new();

    // 递归遍历 Trie
    analyze_node(
        &trie.root,
        trie,
        total_ranks,
        config.minority_threshold,
        0, // depth
        &mut rank_scores,
    );

    // 构建结果
    let mut problematic_ranks: Vec<ProblematicRank> = rank_scores
        .into_iter()
        .map(|(rank_id, (score, points))| ProblematicRank {
            rank_id,
            node_ip: None, // 由调用方填充
            issue_reason: None,
            anomaly_score: score,
            divergence_points: points,
        })
        .collect();

    // 按 anomaly_score 降序排列
    problematic_ranks.sort_by(|a, b| b.anomaly_score.cmp(&a.anomaly_score));

    let duration = start.elapsed();

    RankAnalysisResult {
        problematic_ranks,
        total_ranks,
        analysis_duration_ms: duration.as_millis() as u64,
        trigger,
        timestamp: now_secs(),
        minority_threshold: config.minority_threshold,
    }
}

/// 递归分析 Trie 节点的分叉点
fn analyze_node(
    node: &TrieNode,
    trie: &StackTrie,
    total_ranks: u32,
    threshold: f64,
    depth: u32,
    rank_scores: &mut HashMap<u32, (u32, Vec<DivergencePoint>)>,
) {
    // 只在有多个子分支时才检查（分叉点）
    if node.children.len() > 1 {
        // 计算此节点下各分支的覆盖率
        let parent_rank_count = node.ranks.len() as u32;
        // 如果父节点 rank 数量太少则无意义的分析
        if parent_rank_count < 2 {
            return;
        }

        for (&frame_id, child) in &node.children {
            let child_rank_count = child.ranks.len() as u32;
            let coverage = child_rank_count as f64 / parent_rank_count as f64;

            if coverage < threshold && child_rank_count > 0 {
                // 这是少数派分支
                let frame_name = trie.interner.get(frame_id).to_string();
                let majority_count = parent_rank_count - child_rank_count;

                let divergence = DivergencePoint {
                    frame_name,
                    depth: depth + 1,
                    majority_count,
                    minority_count: child_rank_count,
                    minority_coverage: coverage,
                };

                // 标记该分支中的所有 rank
                for rank in child.ranks.iter() {
                    let entry = rank_scores.entry(rank).or_insert_with(|| (0, Vec::new()));
                    entry.0 += 1;
                    entry.1.push(divergence.clone());
                }
            }
        }
    }

    // 递归遍历所有子节点
    for (_frame_id, child) in &node.children {
        analyze_node(child, trie, total_ranks, threshold, depth + 1, rank_scores);
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flamegraph::stack_merger::{merge_stacks, StackTrie};

    fn default_config() -> RankAnalysisConfig {
        RankAnalysisConfig {
            enabled: true,
            minority_threshold: 0.3,
        }
    }

    #[test]
    fn test_no_divergence() {
        // 所有 rank 走相同路径
        let stacks = vec![
            "main;train;forward;backward",
            "main;train;forward;backward",
            "main;train;forward;backward",
            "main;train;forward;backward",
        ];
        let trie = merge_stacks(stacks);
        let result = analyze_trie(&trie, &default_config(), AnalysisTrigger::Manual);

        assert!(
            result.problematic_ranks.is_empty(),
            "All ranks on same path, no problematic ranks"
        );
        assert_eq!(result.total_ranks, 4);
    }

    #[test]
    fn test_single_outlier() {
        // 3 个 rank 走正常路径，1 个 rank 分叉
        let stacks = vec![
            "main;train;nccl_allreduce;wait", // rank 0
            "main;train;nccl_allreduce;wait", // rank 1
            "main;train;nccl_allreduce;wait", // rank 2
            "main;train;compute;stuck_func",  // rank 3 - 异常
        ];
        let trie = merge_stacks(stacks);
        let result = analyze_trie(&trie, &default_config(), AnalysisTrigger::HangDetected);

        assert!(
            !result.problematic_ranks.is_empty(),
            "Should detect rank 3 as problematic"
        );

        // rank 3 应该在问题列表中
        let rank3 = result.problematic_ranks.iter().find(|r| r.rank_id == 3);
        assert!(rank3.is_some(), "Rank 3 should be detected");
        assert!(rank3.unwrap().anomaly_score > 0);

        // rank 0-2 不应该在问题列表中
        for rank_id in 0..3 {
            let normal = result
                .problematic_ranks
                .iter()
                .find(|r| r.rank_id == rank_id);
            assert!(
                normal.is_none(),
                "Rank {} should not be problematic",
                rank_id
            );
        }
    }

    #[test]
    fn test_multiple_outliers() {
        // 8 个 rank，2 个异常
        let stacks = vec![
            "main;train;nccl_allreduce;wait", // rank 0
            "main;train;nccl_allreduce;wait", // rank 1
            "main;train;nccl_allreduce;wait", // rank 2
            "main;train;nccl_allreduce;wait", // rank 3
            "main;train;nccl_allreduce;wait", // rank 4
            "main;train;nccl_allreduce;wait", // rank 5
            "main;train;compute;error_a",     // rank 6 - 异常
            "main;train;compute;error_b",     // rank 7 - 异常
        ];
        let trie = merge_stacks(stacks);
        let result = analyze_trie(&trie, &default_config(), AnalysisTrigger::HangDetected);

        // rank 6 和 7 应该被检测到
        let problematic_ids: Vec<u32> =
            result.problematic_ranks.iter().map(|r| r.rank_id).collect();
        assert!(problematic_ids.contains(&6), "Rank 6 should be problematic");
        assert!(problematic_ids.contains(&7), "Rank 7 should be problematic");
    }

    #[test]
    fn test_50_50_split_no_minority() {
        // 50/50 分裂，两边都不算少数派（都 >= 30% 阈值）
        let stacks = vec![
            "main;path_a;func1",
            "main;path_a;func1",
            "main;path_b;func2",
            "main;path_b;func2",
        ];
        let trie = merge_stacks(stacks);
        let result = analyze_trie(&trie, &default_config(), AnalysisTrigger::Manual);

        // 50% >= 30% 阈值，两边都不算少数派
        assert!(
            result.problematic_ranks.is_empty(),
            "50/50 split should not flag any rank as problematic with 30% threshold"
        );
    }

    #[test]
    fn test_empty_trie() {
        let trie = StackTrie::with_total_ranks(0);
        let result = analyze_trie(&trie, &default_config(), AnalysisTrigger::Manual);

        assert!(result.problematic_ranks.is_empty());
        assert_eq!(result.total_ranks, 0);
    }

    #[test]
    fn test_divergence_point_info() {
        // 验证分叉点信息的正确性
        let stacks = vec![
            "main;train;nccl_allreduce", // rank 0
            "main;train;nccl_allreduce", // rank 1
            "main;train;nccl_allreduce", // rank 2
            "main;train;nccl_allreduce", // rank 3
            "main;train;nccl_allreduce", // rank 4
            "main;train;nccl_allreduce", // rank 5
            "main;train;nccl_allreduce", // rank 6
            "main;train;nccl_allreduce", // rank 7
            "main;train;nccl_allreduce", // rank 8
            "main;train;compute_stuck",  // rank 9 - 异常 (1/10 = 10% < 30%)
        ];
        let trie = merge_stacks(stacks);
        let result = analyze_trie(&trie, &default_config(), AnalysisTrigger::Manual);

        let rank9 = result.problematic_ranks.iter().find(|r| r.rank_id == 9);
        assert!(rank9.is_some());

        let rank9 = rank9.unwrap();
        assert!(!rank9.divergence_points.is_empty());

        let dp = &rank9.divergence_points[0];
        assert_eq!(dp.minority_count, 1);
        assert_eq!(dp.majority_count, 9);
        assert!(dp.minority_coverage < 0.3);
    }

    #[test]
    fn test_custom_threshold() {
        // 10 个 rank，2 个走不同路径 (20%)
        let stacks = vec![
            "main;path_a",
            "main;path_a",
            "main;path_a",
            "main;path_a",
            "main;path_a",
            "main;path_a",
            "main;path_a",
            "main;path_a",
            "main;path_b",
            "main;path_b",
        ];
        let trie = merge_stacks(stacks);

        // 使用 30% 阈值：20% < 30%，应检测到
        let config30 = RankAnalysisConfig {
            enabled: true,
            minority_threshold: 0.3,
        };
        let result30 = analyze_trie(&trie, &config30, AnalysisTrigger::Manual);
        assert!(
            !result30.problematic_ranks.is_empty(),
            "20% < 30% threshold, should detect"
        );

        // 使用 15% 阈值：20% > 15%，不应检测到
        let config15 = RankAnalysisConfig {
            enabled: true,
            minority_threshold: 0.15,
        };
        let result15 = analyze_trie(&trie, &config15, AnalysisTrigger::Manual);
        assert!(
            result15.problematic_ranks.is_empty(),
            "20% > 15% threshold, should not detect"
        );
    }

    #[test]
    fn test_results_sorted_by_score() {
        // 构造多层分叉，使某些 rank 有更高的 anomaly_score
        let stacks = vec![
            "main;train;nccl;wait",     // rank 0
            "main;train;nccl;wait",     // rank 1
            "main;train;nccl;wait",     // rank 2
            "main;train;nccl;wait",     // rank 3
            "main;train;nccl;wait",     // rank 4
            "main;train;nccl;wait",     // rank 5
            "main;train;nccl;wait",     // rank 6
            "main;train;nccl;wait",     // rank 7
            "main;train;nccl;wait",     // rank 8
            "main;train;compute;error", // rank 9 - 异常
        ];
        let trie = merge_stacks(stacks);
        let result = analyze_trie(&trie, &default_config(), AnalysisTrigger::Manual);

        if result.problematic_ranks.len() > 1 {
            for i in 0..result.problematic_ranks.len() - 1 {
                assert!(
                    result.problematic_ranks[i].anomaly_score
                        >= result.problematic_ranks[i + 1].anomaly_score,
                    "Results should be sorted by anomaly_score descending"
                );
            }
        }
    }

    #[test]
    fn test_parallel_trie_analysis() {
        use crate::flamegraph::stack_merger::parallel_merge_stacks;

        let stacks: Vec<(u32, String)> = vec![
            (0, "main;train;nccl;wait".to_string()),
            (1, "main;train;nccl;wait".to_string()),
            (2, "main;train;nccl;wait".to_string()),
            (3, "main;train;nccl;wait".to_string()),
            (4, "main;train;compute;stuck".to_string()),
        ];
        let trie = parallel_merge_stacks(stacks, Some(2));
        let result = analyze_trie(&trie, &default_config(), AnalysisTrigger::HangDetected);

        let rank4 = result.problematic_ranks.iter().find(|r| r.rank_id == 4);
        assert!(
            rank4.is_some(),
            "Rank 4 should be detected from parallel trie"
        );
    }
}
