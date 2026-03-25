use rand::{thread_rng, Rng, seq::SliceRandom};
use std::collections::HashMap;

/// 火焰图数据生成器，用于性能测试
pub struct FlameGraphDataGenerator {
    function_names: Vec<String>,
    max_depth: usize,
    branching_factor: usize,
}

impl FlameGraphDataGenerator {
    pub fn new(max_depth: usize, branching_factor: usize) -> Self {
        let function_names = Self::generate_realistic_function_names();
        Self {
            function_names,
            max_depth,
            branching_factor,
        }
    }

    /// 生成逼真的函数名列表
    fn generate_realistic_function_names() -> Vec<String> {
        let prefixes = vec![
            "cuda", "nccl", "torch", "allreduce", "allgather", "broadcast", "reduce_scatter",
            "forward", "backward", "optimizer", "gradient", "loss", "activation", "linear",
            "conv", "batch_norm", "dropout", "attention", "transformer", "embedding",
        ];
        
        let suffixes = vec![
            "kernel", "launch", "sync", "async", "compute", "memory", "transfer", "wait",
            "execute", "process", "handle", "callback", "init", "finalize", "update",
        ];

        let mut names = Vec::new();
        
        // 基础函数名
        for prefix in &prefixes {
            for suffix in &suffixes {
                names.push(format!("{}_{}", prefix, suffix));
                names.push(format!("_{}_impl", prefix));
                names.push(format!("{}_{}_{}", prefix, suffix, "v2"));
            }
        }

        // 添加一些深度调用栈
        names.extend(vec![
            "main".to_string(),
            "train_epoch".to_string(),
            "forward_pass".to_string(),
            "backward_pass".to_string(),
            "parameter_update".to_string(),
            "__cuda_runtime_api_wrapper".to_string(),
            "__device_synchronize".to_string(),
            "nccl_all_reduce_ring".to_string(),
        ]);

        names
    }

    /// 为指定数量的rank生成火焰图数据
    pub fn generate_flamegraph_data(&self, rank_count: u32) -> HashMap<u32, String> {
        let mut data = HashMap::new();
        let mut rng = thread_rng();

        for rank in 0..rank_count {
            // 每个rank生成不同的调用栈，但保持一些共同的模式
            let stack = self.generate_single_stack(&mut rng, rank);
            data.insert(rank, stack);
        }

        data
    }

    /// 生成单个rank的调用栈
    fn generate_single_stack(&self, rng: &mut impl Rng, rank: u32) -> String {
        let depth = rng.gen_range(15..=self.max_depth);
        let mut stack = Vec::new();
        
        // 添加一些共同的基础调用栈
        stack.push("main".to_string());
        
        if rank % 4 == 0 {
            stack.push("train_epoch".to_string());
            stack.push("forward_pass".to_string());
        } else if rank % 4 == 1 {
            stack.push("train_epoch".to_string());
            stack.push("backward_pass".to_string());
        } else if rank % 4 == 2 {
            stack.push("train_epoch".to_string());
            stack.push("parameter_update".to_string());
        } else {
            stack.push("sync_all_reduce".to_string());
        }

        // 生成剩余的调用栈
        while stack.len() < depth {
            let func = self.function_names.choose(rng).unwrap();
            
            // 添加一些随机性，某些函数更容易出现在特定深度
            if stack.len() > 10 && rng.gen_bool(0.3) {
                // 深层调用更可能是CUDA相关
                let cuda_funcs: Vec<_> = self.function_names.iter()
                    .filter(|f| f.contains("cuda") || f.contains("nccl") || f.contains("kernel"))
                    .collect();
                if !cuda_funcs.is_empty() {
                    stack.push(cuda_funcs.choose(rng).unwrap().to_string());
                    continue;
                }
            }
            
            stack.push(func.clone());
        }

        // 有一定概率添加重复的函数调用（模拟递归或循环调用）
        if rng.gen_bool(0.2) && stack.len() > 5 {
            let idx = rng.gen_range(2..stack.len());
            let func = stack[idx].clone();
            stack.push(func);
        }

        stack.join(";")
    }

    /// 生成符合HTTP响应格式的火焰图数据
    pub fn generate_http_response_data(&self, rank_count: u32) -> Vec<String> {
        let flamegraph_data = self.generate_flamegraph_data(rank_count);
        
        flamegraph_data.iter()
            .map(|(rank, stack)| {
                // 模拟真实API响应格式
                format!("rank_{}: {}", rank, stack)
            })
            .collect()
    }

    /// 生成大规模测试数据集，用于压力测试
    pub fn generate_large_dataset(&self, total_ranks: u32, batch_size: u32) -> Vec<Vec<(u32, String)>> {
        let mut batches = Vec::new();
        let mut current_batch = Vec::new();
        
        let flamegraph_data = self.generate_flamegraph_data(total_ranks);
        
        for (rank, stack) in flamegraph_data {
            current_batch.push((rank, stack));
            
            if current_batch.len() >= batch_size as usize {
                batches.push(current_batch);
                current_batch = Vec::new();
            }
        }
        
        if !current_batch.is_empty() {
            batches.push(current_batch);
        }
        
        batches
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_flamegraph_data() {
        let generator = FlameGraphDataGenerator::new(30, 5);
        let data = generator.generate_flamegraph_data(100);
        
        assert_eq!(data.len(), 100);
        
        for (rank, stack) in data.iter() {
            assert!(*rank < 100);
            assert!(!stack.is_empty());
            assert!(stack.contains(";"));
            
            let frames: Vec<&str> = stack.split(';').collect();
            assert!(frames.len() >= 15);
            assert!(frames.len() <= 30);
        }
    }

    #[test]
    fn test_generate_large_dataset() {
        let generator = FlameGraphDataGenerator::new(25, 3);
        let batches = generator.generate_large_dataset(1000, 100);
        
        assert_eq!(batches.len(), 10);
        
        let total_items: usize = batches.iter().map(|b| b.len()).sum();
        assert_eq!(total_items, 1000);
        
        for batch in batches {
            assert!(batch.len() <= 100);
            assert!(!batch.is_empty());
        }
    }

    #[test]
    fn test_function_name_diversity() {
        let generator = FlameGraphDataGenerator::new(20, 3);
        let data = generator.generate_flamegraph_data(50);
        
        // 检查生成的函数名有足够的多样性
        let mut all_functions = std::collections::HashSet::new();
        for stack in data.values() {
            for func in stack.split(';') {
                all_functions.insert(func);
            }
        }
        
        // 应该有足够多的不同函数名
        assert!(all_functions.len() > 20);
    }
}