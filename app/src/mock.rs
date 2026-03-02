use crate::models::*;
use std::time::{SystemTime, UNIX_EPOCH};

const NODE_COUNT: usize = 16;
const GPUS_PER_NODE: usize = 8;
const TOTAL_RANKS: usize = NODE_COUNT * GPUS_PER_NODE;

/// 简单的伪随机数生成器 (LCG)
struct SimpleRng {
    state: u64,
}

impl SimpleRng {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_mul(6364136223846793005).wrapping_add(1);
        self.state
    }

    fn next_f64(&mut self) -> f64 {
        (self.next_u64() as f64) / (u64::MAX as f64)
    }

    fn next_range(&mut self, min: f64, max: f64) -> f64 {
        min + self.next_f64() * (max - min)
    }

    fn next_bool(&mut self, probability: f64) -> bool {
        self.next_f64() < probability
    }
}

/// 获取当前时间戳
fn now_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// 生成节点 IP
fn node_ip(index: usize) -> String {
    format!("192.168.1.{}", index + 1)
}

/// 生成主机名
fn hostname(index: usize) -> String {
    format!("node-{:02}", index + 1)
}

/// 生成机柜 ID (每 4 台一组)
fn rack_id(node_index: usize) -> String {
    format!("rack-{:02}", (node_index / 4) + 1)
}

/// 生成所有 Rank 的 Mock 数据
pub fn generate_all_ranks() -> Vec<RankMetrics> {
    let seed = now_timestamp();
    let mut rng = SimpleRng::new(seed);
    let mut ranks = Vec::with_capacity(TOTAL_RANKS);

    // 先计算全局 P50 用于 ratio 计算
    let global_p50 = 100.0; // 基准值

    for node_idx in 0..NODE_COUNT {
        let ip = node_ip(node_idx);

        for local_rank in 0..GPUS_PER_NODE {
            let rank_id = (node_idx * GPUS_PER_NODE + local_rank) as u32;

            // 决定这个 rank 的类型：正常 (88%)、慢 (10%)、故障 (2%)
            let roll = rng.next_f64();
            let (status, step_time_ms, gpu_util, error_msg) = if roll < 0.02 {
                // 故障
                (
                    HealthStatus::Critical,
                    rng.next_range(500.0, 1000.0),
                    rng.next_range(0.0, 20.0) as f32,
                    Some(pick_error(&mut rng)),
                )
            } else if roll < 0.12 {
                // 慢
                (
                    HealthStatus::Warning,
                    rng.next_range(200.0, 500.0),
                    rng.next_range(40.0, 70.0) as f32,
                    None,
                )
            } else {
                // 正常
                (
                    HealthStatus::Healthy,
                    rng.next_range(80.0, 120.0),
                    rng.next_range(85.0, 98.0) as f32,
                    None,
                )
            };

            let rank = RankMetrics {
                rank_id,
                local_rank: local_rank as u8,
                node_ip: ip.clone(),
                step_time_ms,
                step_time_ratio: step_time_ms / global_p50,
                gpu_utilization: gpu_util,
                gpu_memory_used_gb: rng.next_range(60.0, 78.0) as f32,
                gpu_memory_total_gb: 80.0,
                nccl_latency_ms: if status == HealthStatus::Critical {
                    rng.next_range(10.0, 50.0)
                } else {
                    rng.next_range(0.5, 2.0)
                },
                nccl_bandwidth_gbps: rng.next_range(150.0, 200.0) as f32,
                status,
                last_heartbeat: if status == HealthStatus::Critical && rng.next_bool(0.5) {
                    now_timestamp() - rng.next_range(60.0, 300.0) as u64
                } else {
                    now_timestamp() - rng.next_range(0.0, 5.0) as u64
                },
                current_step: 10000 + rng.next_range(0.0, 100.0) as u64,
                error_message: error_msg,
            };

            ranks.push(rank);
        }
    }

    ranks
}

fn pick_error(rng: &mut SimpleRng) -> String {
    let errors = [
        "NCCL timeout",
        "OOM: Out of memory",
        "Heartbeat lost",
        "CUDA error",
        "NVLink failure",
    ];
    let idx = (rng.next_u64() as usize) % errors.len();
    errors[idx].to_string()
}

/// 从 Rank 数据聚合生成节点数据
pub fn aggregate_nodes(ranks: &[RankMetrics]) -> Vec<NodeMetrics> {
    let mut nodes: Vec<NodeMetrics> = Vec::with_capacity(NODE_COUNT);

    for node_idx in 0..NODE_COUNT {
        let ip = node_ip(node_idx);
        let node_ranks: Vec<&RankMetrics> = ranks.iter().filter(|r| r.node_ip == ip).collect();

        if node_ranks.is_empty() {
            continue;
        }

        let rank_count = node_ranks.len() as u8;
        let healthy_count = node_ranks
            .iter()
            .filter(|r| r.status == HealthStatus::Healthy)
            .count() as u8;
        let warning_count = node_ranks
            .iter()
            .filter(|r| r.status == HealthStatus::Warning)
            .count() as u8;
        let critical_count = node_ranks
            .iter()
            .filter(|r| r.status == HealthStatus::Critical)
            .count() as u8;

        // 计算性能聚合
        let mut step_times: Vec<f64> = node_ranks.iter().map(|r| r.step_time_ms).collect();
        step_times.sort_by(|a, b| a.partial_cmp(b).unwrap());

        let avg_step_time_ms = step_times.iter().sum::<f64>() / step_times.len() as f64;
        let p50_step_time_ms = percentile(&step_times, 50);
        let p99_step_time_ms = percentile(&step_times, 99);

        let avg_gpu_utilization =
            node_ranks.iter().map(|r| r.gpu_utilization).sum::<f32>() / rank_count as f32;
        let avg_nccl_latency_ms =
            node_ranks.iter().map(|r| r.nccl_latency_ms).sum::<f64>() / rank_count as f64;

        // 慢占比：step_time > 150ms (即 P50 * 1.5)
        let slow_count = node_ranks.iter().filter(|r| r.step_time_ms > 150.0).count();
        let slow_ratio = slow_count as f32 / rank_count as f32;

        // 节点状态：取最严重的
        let status = if critical_count > 0 {
            HealthStatus::Critical
        } else if warning_count > 0 {
            HealthStatus::Warning
        } else {
            HealthStatus::Healthy
        };

        nodes.push(NodeMetrics {
            node_ip: ip,
            hostname: hostname(node_idx),
            rack_id: rack_id(node_idx),
            rank_count,
            healthy_count,
            warning_count,
            critical_count,
            slow_ratio,
            avg_step_time_ms,
            p50_step_time_ms,
            p99_step_time_ms,
            avg_gpu_utilization,
            avg_nccl_latency_ms,
            status,
            last_update: now_timestamp(),
        });
    }

    nodes
}

fn percentile(sorted: &[f64], p: usize) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = (sorted.len() * p / 100).min(sorted.len() - 1);
    sorted[idx]
}

/// 生成全局聚合数据
pub fn generate_global_metrics(nodes: &[NodeMetrics], ranks: &[RankMetrics]) -> GlobalMetrics {
    let total_nodes = nodes.len() as u16;
    let total_ranks = ranks.len() as u16;

    let healthy_nodes = nodes
        .iter()
        .filter(|n| n.status == HealthStatus::Healthy)
        .count() as u16;
    let warning_nodes = nodes
        .iter()
        .filter(|n| n.status == HealthStatus::Warning)
        .count() as u16;
    let critical_nodes = nodes
        .iter()
        .filter(|n| n.status == HealthStatus::Critical)
        .count() as u16;

    let healthy_ranks = ranks
        .iter()
        .filter(|r| r.status == HealthStatus::Healthy)
        .count() as u16;
    let warning_ranks = ranks
        .iter()
        .filter(|r| r.status == HealthStatus::Warning)
        .count() as u16;
    let critical_ranks = ranks
        .iter()
        .filter(|r| r.status == HealthStatus::Critical)
        .count() as u16;

    let mut all_step_times: Vec<f64> = ranks.iter().map(|r| r.step_time_ms).collect();
    all_step_times.sort_by(|a, b| a.partial_cmp(b).unwrap());

    let global_p50_step_time_ms = percentile(&all_step_times, 50);
    let global_p99_step_time_ms = percentile(&all_step_times, 99);
    let global_avg_gpu_utilization =
        ranks.iter().map(|r| r.gpu_utilization).sum::<f32>() / total_ranks as f32;

    let slow_node_count = nodes.iter().filter(|n| n.slow_ratio > 0.0).count();
    let slow_node_ratio = slow_node_count as f32 / total_nodes as f32;

    GlobalMetrics {
        total_nodes,
        total_ranks,
        healthy_nodes,
        warning_nodes,
        critical_nodes,
        healthy_ranks,
        warning_ranks,
        critical_ranks,
        global_p50_step_time_ms,
        global_p99_step_time_ms,
        global_avg_gpu_utilization,
        slow_node_ratio,
        current_step: 10050,
        steps_per_second: 0.95,
        estimated_remaining_hours: Some(24.5),
        last_update: now_timestamp(),
    }
}

/// 生成拓扑数据
pub fn generate_topology(nodes: &[NodeMetrics]) -> Topology {
    let mut racks: Vec<RackInfo> = Vec::new();

    for rack_num in 1..=4 {
        let rack_id = format!("rack-{:02}", rack_num);
        let rack_nodes: Vec<NodeSummary> = nodes
            .iter()
            .filter(|n| n.rack_id == rack_id)
            .map(|n| NodeSummary {
                node_ip: n.node_ip.clone(),
                status: n.status,
                slow_ratio: n.slow_ratio,
            })
            .collect();

        racks.push(RackInfo {
            rack_id,
            nodes: rack_nodes,
        });
    }

    Topology { racks }
}

/// Mock 数据管理器
pub struct MockDataStore {
    pub ranks: Vec<RankMetrics>,
    pub nodes: Vec<NodeMetrics>,
    pub global: GlobalMetrics,
    pub topology: Topology,
}

impl MockDataStore {
    pub fn new() -> Self {
        let ranks = generate_all_ranks();
        let nodes = aggregate_nodes(&ranks);
        let global = generate_global_metrics(&nodes, &ranks);
        let topology = generate_topology(&nodes);

        Self {
            ranks,
            nodes,
            global,
            topology,
        }
    }

    pub fn get_ranks_by_ip(&self, ip: &str) -> Vec<RankMetrics> {
        self.ranks
            .iter()
            .filter(|r| r.node_ip == ip)
            .cloned()
            .collect()
    }

    pub fn get_node_by_ip(&self, ip: &str) -> Option<NodeMetrics> {
        self.nodes.iter().find(|n| n.node_ip == ip).cloned()
    }
}

impl Default for MockDataStore {
    fn default() -> Self {
        Self::new()
    }
}

// ============ Mock 堆栈数据生成 ============

/// 模拟的调用栈模板
const COMMON_STACK_FRAMES: &[&str] = &[
    "main",
    "torch::distributed::init_process_group",
    "training_loop",
    "model.forward",
    "transformer_block",
    "attention_layer",
    "torch::matmul",
];

const NCCL_STACK_FRAMES: &[&str] = &[
    "nccl::AllReduce",
    "nccl::internal::enqueue",
    "nccl::net::send",
];

const SLOW_STACK_FRAMES: &[&str] = &[
    "cudaStreamSynchronize",
    "cudaMemcpyAsync",
    "data_loader::next_batch",
    "disk_io::read_chunk",
];

const COMPUTE_STACK_FRAMES: &[&str] = &[
    "cublas::gemm",
    "cudnn::convolution_forward",
    "torch::autograd::backward",
];

/// 为指定节点生成 mock 堆栈数据
pub fn generate_node_stacks(node_ip: &str, ranks: &[RankMetrics]) -> Vec<RankStack> {
    let seed = now_timestamp() ^ (node_ip.as_bytes().iter().map(|&b| b as u64).sum::<u64>());
    let mut rng = SimpleRng::new(seed);

    ranks
        .iter()
        .filter(|r| r.node_ip == node_ip)
        .map(|rank| {
            let callstack = generate_mock_callstack(&mut rng, rank.status);
            RankStack {
                rank_id: rank.rank_id,
                node_ip: node_ip.to_string(),
                callstack,
                timestamp: now_timestamp(),
            }
        })
        .collect()
}

fn generate_mock_callstack(rng: &mut SimpleRng, status: HealthStatus) -> Vec<String> {
    let mut stack: Vec<String> = COMMON_STACK_FRAMES.iter().map(|s| s.to_string()).collect();

    match status {
        HealthStatus::Critical => {
            // 故障 rank: 卡在 NCCL 通信
            stack.extend(NCCL_STACK_FRAMES.iter().map(|s| s.to_string()));
            stack.push("nccl::wait_timeout".to_string());
        }
        HealthStatus::Warning => {
            // 慢 rank: 在 IO 或同步上
            if rng.next_bool(0.5) {
                stack.extend(SLOW_STACK_FRAMES.iter().map(|s| s.to_string()));
            } else {
                stack.extend(NCCL_STACK_FRAMES.iter().map(|s| s.to_string()));
            }
        }
        HealthStatus::Healthy => {
            // 正常 rank: 在计算中
            stack.extend(COMPUTE_STACK_FRAMES.iter().map(|s| s.to_string()));
            // 随机添加一些变化
            if rng.next_bool(0.3) {
                stack.push("torch::nn::LayerNorm::forward".to_string());
            }
            if rng.next_bool(0.3) {
                stack.push("torch::nn::Dropout::forward".to_string());
            }
        }
    }

    stack
}

/// 合并多个 rank 的堆栈为树结构
pub fn merge_stacks(stacks: &[RankStack]) -> MergedStackFrame {
    let total_ranks = stacks.len() as u32;
    let mut root = MergedStackFrame {
        frame_name: "root".to_string(),
        depth: 0,
        rank_ids: Vec::new(),
        rank_count: 0,
        total_ranks,
        children: Vec::new(),
    };

    for stack in stacks {
        insert_stack(&mut root, &stack.callstack, stack.rank_id, total_ranks, 0);
    }

    // 根节点包含所有 rank
    root.rank_ids = stacks.iter().map(|s| s.rank_id).collect();
    root.rank_count = total_ranks;

    root
}

fn insert_stack(node: &mut MergedStackFrame, frames: &[String], rank_id: u32, total_ranks: u32, depth: u32) {
    if frames.is_empty() {
        return;
    }

    let frame_name = &frames[0];
    let remaining = &frames[1..];

    // 查找或创建子节点
    let child = if let Some(existing) = node.children.iter_mut().find(|c| &c.frame_name == frame_name) {
        existing
    } else {
        node.children.push(MergedStackFrame {
            frame_name: frame_name.clone(),
            depth: depth + 1,
            rank_ids: Vec::new(),
            rank_count: 0,
            total_ranks,
            children: Vec::new(),
        });
        node.children.last_mut().unwrap()
    };

    // 添加 rank_id
    if !child.rank_ids.contains(&rank_id) {
        child.rank_ids.push(rank_id);
        child.rank_count += 1;
    }

    // 递归处理剩余帧
    insert_stack(child, remaining, rank_id, total_ranks, depth + 1);
}
