//! Megatron mixed-radix 拓扑与问题 Rank 关联分析。

use std::collections::HashMap;

use crate::rank_analysis_types::{
    ParallelDimension, ParallelEvidence, ParallelEvidenceKind, ParallelTopologySummary,
    RankAnalysisResult, RankParallelContext, RootCauseConfidence,
};

const DEFAULT_ORDER: &str = "tp-cp-ep-dp-pp";
const DIMENSIONS: [ParallelDimension; 5] = [
    ParallelDimension::PP,
    ParallelDimension::DP,
    ParallelDimension::TP,
    ParallelDimension::EP,
    ParallelDimension::CP,
];

#[derive(Debug, Clone)]
pub struct ParallelTopology {
    summary: ParallelTopologySummary,
    contexts: Vec<RankParallelContext>,
    groups: HashMap<(ParallelDimension, u32), Vec<u32>>,
}

#[derive(Debug, Clone)]
struct Grid {
    order: Vec<String>,
    sizes: HashMap<String, u32>,
    world_size: u32,
}

impl Grid {
    fn new(order: &str, sizes: HashMap<String, u32>, world_size: u32) -> Result<Self, String> {
        let order = normalize_order(order, &sizes)?;
        let product = order.iter().try_fold(1u32, |acc, name| {
            acc.checked_mul(*sizes.get(name).unwrap_or(&1))
                .ok_or_else(|| "并行度乘积溢出".to_string())
        })?;
        if product != world_size {
            return Err(format!(
                "并行度乘积 {} 与 world_size {} 不一致",
                product, world_size
            ));
        }
        Ok(Self {
            order,
            sizes,
            world_size,
        })
    }

    fn coordinates(&self, rank: u32) -> HashMap<String, u32> {
        let mut stride = 1u32;
        let mut result = HashMap::new();
        for name in &self.order {
            let size = *self.sizes.get(name).unwrap_or(&1);
            result.insert(name.clone(), (rank / stride) % size);
            stride *= size;
        }
        result
    }

    fn groups(&self, dimension: &str) -> Vec<Vec<u32>> {
        let mut grouped: HashMap<Vec<(String, u32)>, Vec<u32>> = HashMap::new();
        for rank in 0..self.world_size {
            let coordinates = self.coordinates(rank);
            let key = self
                .order
                .iter()
                .filter(|name| name.as_str() != dimension)
                .map(|name| (name.clone(), *coordinates.get(name).unwrap_or(&0)))
                .collect::<Vec<_>>();
            grouped.entry(key).or_default().push(rank);
        }
        let mut groups = grouped.into_values().collect::<Vec<_>>();
        groups.sort_by_key(|group| group.first().copied().unwrap_or(u32::MAX));
        groups
    }
}

impl ParallelTopology {
    /// 当 DP 显式配置时，可仅根据进程环境恢复 dense world_size。
    pub fn configured_world_size(env: &HashMap<String, String>) -> Result<Option<u32>, String> {
        let Some(dp) = parse_optional_size(env, "DP")? else {
            return Ok(None);
        };
        let tp = parse_size(env, "TP")?;
        let pp = parse_size(env, "PP")?;
        let cp = parse_size(env, "CP")?;
        tp.checked_mul(pp)
            .and_then(|value| value.checked_mul(cp))
            .and_then(|value| value.checked_mul(dp))
            .map(Some)
            .ok_or_else(|| "TP×PP×CP×DP 溢出".to_string())
    }

    pub fn from_env_map(env: &HashMap<String, String>, world_size: u32) -> Result<Self, String> {
        if world_size == 0 {
            return Err("world_size 为 0".to_string());
        }
        if ![
            "TP",
            "PP",
            "CP",
            "EP",
            "DP",
            "TP_SIZE",
            "PP_SIZE",
            "CP_SIZE",
            "EP_SIZE",
            "DP_SIZE",
            "RANK_ORDER",
        ]
        .iter()
        .any(|name| env.contains_key(*name))
        {
            return Err(
                "进程环境变量中没有 TP/PP/DP/EP/CP 或 RANK_ORDER，未结合并行拓扑".to_string(),
            );
        }

        let tp = parse_size(env, "TP")?;
        let pp = parse_size(env, "PP")?;
        let cp = parse_size(env, "CP")?;
        let ep = parse_size(env, "EP")?;
        let dense_model_size = tp
            .checked_mul(pp)
            .and_then(|v| v.checked_mul(cp))
            .ok_or_else(|| "TP×PP×CP 溢出".to_string())?;
        if world_size % dense_model_size != 0 {
            return Err(format!(
                "world_size {} 不能被 TP×PP×CP={} 整除",
                world_size, dense_model_size
            ));
        }
        let dp = world_size / dense_model_size;
        if let Some(explicit_dp) = parse_optional_size(env, "DP")? {
            if explicit_dp != dp {
                return Err(format!(
                    "DP={}，但按 world_size 推导应为 {}",
                    explicit_dp, dp
                ));
            }
        }

        let expert_model_size = tp
            .checked_mul(pp)
            .and_then(|v| v.checked_mul(ep))
            .ok_or_else(|| "TP×PP×EP 溢出".to_string())?;
        if world_size % expert_model_size != 0 {
            return Err(format!(
                "world_size {} 不能被 TP×PP×EP={} 整除",
                world_size, expert_model_size
            ));
        }
        let expert_dp = world_size / expert_model_size;
        let order = env
            .get("RANK_ORDER")
            .map(|value| value.trim().to_ascii_lowercase())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| DEFAULT_ORDER.to_string());

        let dense = Grid::new(
            &order,
            HashMap::from([
                ("tp".to_string(), tp),
                ("pp".to_string(), pp),
                ("dp".to_string(), dp),
                ("cp".to_string(), cp),
                ("ep".to_string(), 1),
            ]),
            world_size,
        )?;
        let expert = Grid::new(
            &order,
            HashMap::from([
                ("tp".to_string(), tp),
                ("pp".to_string(), pp),
                ("dp".to_string(), expert_dp),
                ("cp".to_string(), 1),
                ("ep".to_string(), ep),
            ]),
            world_size,
        )?;

        let mut contexts = Vec::with_capacity(world_size as usize);
        for rank in 0..world_size {
            let dense_coordinates = dense.coordinates(rank);
            let expert_coordinates = expert.coordinates(rank);
            contexts.push(RankParallelContext {
                pp_rank: dense_coordinates.get("pp").copied(),
                dp_rank: dense_coordinates.get("dp").copied(),
                tp_rank: dense_coordinates.get("tp").copied(),
                ep_rank: expert_coordinates.get("ep").copied(),
                cp_rank: dense_coordinates.get("cp").copied(),
                expert_dp_rank: expert_coordinates.get("dp").copied(),
            });
        }

        let mut groups = HashMap::new();
        insert_groups(&mut groups, ParallelDimension::TP, dense.groups("tp"));
        insert_groups(&mut groups, ParallelDimension::PP, dense.groups("pp"));
        insert_groups(&mut groups, ParallelDimension::DP, dense.groups("dp"));
        insert_groups(&mut groups, ParallelDimension::CP, dense.groups("cp"));
        insert_groups(&mut groups, ParallelDimension::EP, expert.groups("ep"));

        Ok(Self {
            summary: ParallelTopologySummary {
                available: true,
                framework: "megatron".to_string(),
                tp_size: tp,
                pp_size: pp,
                dp_size: dp,
                ep_size: ep,
                cp_size: cp,
                rank_order: order,
                degraded_reason: None,
            },
            contexts,
            groups,
        })
    }

    pub fn summary(&self) -> ParallelTopologySummary {
        self.summary.clone()
    }

    fn context(&self, rank: u32) -> Option<RankParallelContext> {
        self.contexts.get(rank as usize).cloned()
    }

    fn group(&self, dimension: ParallelDimension, rank: u32) -> Option<&Vec<u32>> {
        self.groups.get(&(dimension, rank))
    }
}

fn insert_groups(
    destination: &mut HashMap<(ParallelDimension, u32), Vec<u32>>,
    dimension: ParallelDimension,
    groups: Vec<Vec<u32>>,
) {
    for group in groups {
        for rank in &group {
            destination.insert((dimension, *rank), group.clone());
        }
    }
}

fn parse_size(env: &HashMap<String, String>, name: &str) -> Result<u32, String> {
    Ok(parse_optional_size(env, name)?.unwrap_or(1))
}

fn parse_optional_size(env: &HashMap<String, String>, name: &str) -> Result<Option<u32>, String> {
    let alias = format!("{}_SIZE", name);
    let Some(raw) = env.get(name).or_else(|| env.get(&alias)) else {
        return Ok(None);
    };
    let size = raw
        .trim()
        .parse::<u32>()
        .map_err(|_| format!("{}={} 不是有效正整数", name, raw))?;
    if size == 0 {
        return Err(format!("{} 必须大于 0", name));
    }
    Ok(Some(size))
}

fn normalize_order(order: &str, sizes: &HashMap<String, u32>) -> Result<Vec<String>, String> {
    let mut tokens = Vec::new();
    for token in order
        .split('-')
        .map(str::trim)
        .filter(|token| !token.is_empty())
    {
        if !matches!(token, "tp" | "pp" | "dp" | "ep" | "cp") {
            return Err(format!("RANK_ORDER 包含未知维度 {}", token));
        }
        if tokens.iter().any(|existing| existing == token) {
            return Err(format!("RANK_ORDER 重复包含维度 {}", token));
        }
        tokens.push(token.to_string());
    }
    for name in ["tp", "cp", "ep", "dp", "pp"] {
        if !tokens.iter().any(|token| token == name) {
            if *sizes.get(name).unwrap_or(&1) != 1 {
                return Err(format!("{} 并行度大于 1，但 RANK_ORDER 未包含它", name));
            }
            tokens.push(name.to_string());
        }
    }
    Ok(tokens)
}

pub fn correlate_result(
    result: &mut RankAnalysisResult,
    topology: Option<&ParallelTopology>,
    degraded_reason: Option<String>,
) {
    let signatures = result
        .problematic_ranks
        .iter()
        .map(|rank| (rank.rank_id, rank.divergence_points.clone()))
        .collect::<HashMap<_, _>>();

    if let Some(topology) = topology {
        result.parallel_topology = topology.summary();
    } else {
        result.parallel_topology = ParallelTopologySummary {
            degraded_reason: Some(
                degraded_reason.unwrap_or_else(|| "未获取到 Megatron 并行拓扑".to_string()),
            ),
            ..ParallelTopologySummary::default()
        };
    }

    for rank in &mut result.problematic_ranks {
        rank.parallel_context = topology.and_then(|value| value.context(rank.rank_id));
        rank.parallel_evidence.clear();
        rank.suspected_dimensions.clear();

        if rank.issue_reason.is_some() {
            rank.root_cause_confidence = RootCauseConfidence::High;
            continue;
        }

        let Some(topology) = topology else {
            rank.root_cause_confidence = RootCauseConfidence::Low;
            continue;
        };

        let divergence_points = rank.divergence_points.clone();
        for point in &divergence_points {
            for dimension in DIMENSIONS {
                let Some(group) = topology.group(dimension, rank.rank_id) else {
                    continue;
                };
                if group.len() < 2 {
                    continue;
                }
                let matching_ranks = group
                    .iter()
                    .copied()
                    .filter(|peer| {
                        signatures.get(peer).is_some_and(|points| {
                            points.iter().any(|candidate| {
                                candidate.depth == point.depth
                                    && candidate.frame_name == point.frame_name
                            })
                        })
                    })
                    .collect::<Vec<_>>();
                let coverage = matching_ranks.len() as f64 / group.len() as f64;
                let kind = if coverage < result.minority_threshold {
                    Some(ParallelEvidenceKind::GroupOutlier)
                } else if coverage >= 1.0 - result.minority_threshold {
                    Some(ParallelEvidenceKind::GroupCorrelated)
                } else {
                    None
                };
                if let Some(kind) = kind {
                    push_evidence(
                        rank,
                        ParallelEvidence {
                            dimension,
                            kind,
                            group_ranks: group.clone(),
                            matching_ranks: matching_ranks.clone(),
                            coverage,
                            frame_name: point.frame_name.clone(),
                            depth: point.depth,
                        },
                    );
                }
                if dimension == ParallelDimension::DP
                    && matching_ranks.len() >= 2
                    && coverage >= 0.5
                {
                    push_evidence(
                        rank,
                        ParallelEvidence {
                            dimension,
                            kind: ParallelEvidenceKind::ReplicaRepeated,
                            group_ranks: group.clone(),
                            matching_ranks,
                            coverage,
                            frame_name: point.frame_name.clone(),
                            depth: point.depth,
                        },
                    );
                }
            }
        }

        let has_group = rank.parallel_evidence.iter().any(|evidence| {
            matches!(
                evidence.kind,
                ParallelEvidenceKind::GroupOutlier | ParallelEvidenceKind::GroupCorrelated
            )
        });
        let has_replica = rank
            .parallel_evidence
            .iter()
            .any(|evidence| evidence.kind == ParallelEvidenceKind::ReplicaRepeated);
        rank.root_cause_confidence = match (has_group, has_replica) {
            (true, true) => RootCauseConfidence::High,
            (true, false) | (false, true) => RootCauseConfidence::Medium,
            (false, false) => RootCauseConfidence::Low,
        };
        rank.suspected_dimensions.sort();
        rank.suspected_dimensions.dedup();
    }

    result.problematic_ranks.sort_by(|left, right| {
        right
            .issue_reason
            .is_some()
            .cmp(&left.issue_reason.is_some())
            .then_with(|| right.root_cause_confidence.cmp(&left.root_cause_confidence))
            .then_with(|| {
                right
                    .parallel_evidence
                    .len()
                    .cmp(&left.parallel_evidence.len())
            })
            .then_with(|| right.anomaly_score.cmp(&left.anomaly_score))
            .then_with(|| left.rank_id.cmp(&right.rank_id))
    });
}

fn push_evidence(
    rank: &mut crate::rank_analysis_types::ProblematicRank,
    evidence: ParallelEvidence,
) {
    let duplicate = rank.parallel_evidence.iter().any(|existing| {
        existing.dimension == evidence.dimension
            && existing.kind == evidence.kind
            && existing.depth == evidence.depth
            && existing.frame_name == evidence.frame_name
    });
    if !duplicate {
        rank.suspected_dimensions.push(evidence.dimension);
        rank.parallel_evidence.push(evidence);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rank_analysis_types::{AnalysisTrigger, DivergencePoint, ProblematicRank};

    fn env(values: &[(&str, &str)]) -> HashMap<String, String> {
        values
            .iter()
            .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
            .collect()
    }

    #[test]
    fn megatron_mixed_radix_matches_tp_pp_dp_example() {
        let topology = ParallelTopology::from_env_map(
            &env(&[
                ("TP", "2"),
                ("PP", "2"),
                ("DP", "2"),
                ("RANK_ORDER", "tp-pp-dp"),
            ]),
            8,
        )
        .unwrap();
        assert_eq!(
            topology.group(ParallelDimension::DP, 0).unwrap(),
            &vec![0, 4]
        );
        assert_eq!(
            topology.group(ParallelDimension::TP, 0).unwrap(),
            &vec![0, 1]
        );
    }

    #[test]
    fn invalid_explicit_dp_degrades() {
        let error =
            ParallelTopology::from_env_map(&env(&[("TP", "2"), ("DP", "3")]), 8).unwrap_err();
        assert!(error.contains("推导应为 4"));
    }

    #[test]
    fn correlates_group_and_replica_evidence() {
        let topology = ParallelTopology::from_env_map(
            &env(&[("TP", "2"), ("DP", "2"), ("RANK_ORDER", "tp-dp")]),
            4,
        )
        .unwrap();
        let point = DivergencePoint {
            frame_name: "nccl_wait".to_string(),
            depth: 3,
            majority_count: 2,
            minority_count: 2,
            minority_coverage: 0.5,
        };
        let mut result = RankAnalysisResult {
            total_ranks: 4,
            minority_threshold: 0.3,
            trigger: AnalysisTrigger::Manual,
            problematic_ranks: vec![0, 2]
                .into_iter()
                .map(|rank_id| ProblematicRank {
                    rank_id,
                    node_ip: None,
                    issue_reason: None,
                    anomaly_score: 1,
                    divergence_points: vec![point.clone()],
                    parallel_context: None,
                    suspected_dimensions: vec![],
                    root_cause_confidence: RootCauseConfidence::Low,
                    parallel_evidence: vec![],
                })
                .collect(),
            ..Default::default()
        };
        correlate_result(&mut result, Some(&topology), None);
        assert!(result.problematic_ranks.iter().all(|rank| {
            rank.parallel_evidence
                .iter()
                .any(|evidence| evidence.kind == ParallelEvidenceKind::ReplicaRepeated)
        }));
        assert!(result
            .problematic_ranks
            .iter()
            .all(|rank| rank.root_cause_confidence == RootCauseConfidence::High));
    }

    #[test]
    fn builds_separate_dense_cp_and_expert_ep_groups() {
        let topology = ParallelTopology::from_env_map(
            &env(&[("TP", "2"), ("CP", "2"), ("EP", "2"), ("DP", "4")]),
            16,
        )
        .unwrap();
        assert_eq!(topology.group(ParallelDimension::CP, 0).unwrap().len(), 2);
        assert_eq!(topology.group(ParallelDimension::EP, 0).unwrap().len(), 2);
        let context = topology.context(0).unwrap();
        assert_eq!(context.cp_rank, Some(0));
        assert_eq!(context.ep_rank, Some(0));
        assert_eq!(topology.summary.dp_size, 4);
    }

    #[test]
    fn missing_topology_keeps_global_candidate_at_low_confidence() {
        let mut result = RankAnalysisResult {
            total_ranks: 4,
            problematic_ranks: vec![ProblematicRank {
                rank_id: 1,
                node_ip: None,
                issue_reason: None,
                anomaly_score: 2,
                divergence_points: vec![],
                parallel_context: None,
                suspected_dimensions: vec![],
                root_cause_confidence: RootCauseConfidence::High,
                parallel_evidence: vec![],
            }],
            ..Default::default()
        };
        correlate_result(&mut result, None, Some("平台不可用".to_string()));
        assert!(!result.parallel_topology.available);
        assert_eq!(
            result.parallel_topology.degraded_reason.as_deref(),
            Some("平台不可用")
        );
        assert_eq!(
            result.problematic_ranks[0].root_cause_confidence,
            RootCauseConfidence::Low
        );
    }

    #[test]
    fn size_suffix_alias_remains_compatible_but_short_name_has_priority() {
        let topology =
            ParallelTopology::from_env_map(&env(&[("TP", "2"), ("TP_SIZE", "4"), ("DP", "4")]), 8)
                .unwrap();
        assert_eq!(topology.summary.tp_size, 2);
        assert_eq!(topology.summary.dp_size, 4);
    }

    #[test]
    fn derives_complete_world_size_from_explicit_dp() {
        let world_size = ParallelTopology::configured_world_size(&env(&[
            ("TP", "2"),
            ("PP", "3"),
            ("CP", "2"),
            ("DP", "4"),
        ]))
        .unwrap();
        assert_eq!(world_size, Some(48));
    }
}
