use rand::{seq::SliceRandom, thread_rng, Rng};
use std::collections::HashMap;
use std::path::Path;

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

    /// 生成逼真的函数名列表 - 增加长度和复杂度
    fn generate_realistic_function_names() -> Vec<String> {
        let prefixes = vec![
            "cuda_nccl_communicator",
            "torch_distributed_allreduce",
            "torch_distributed_allgather",
            "torch_distributed_broadcast",
            "torch_distributed_reduce_scatter",
            "torch_tensor_forward_pass",
            "torch_tensor_backward_pass",
            "torch_optimizer_parameter_update",
            "torch_nn_linear_layer",
            "torch_nn_conv2d_layer",
            "torch_nn_batch_norm",
            "torch_nn_dropout",
            "torch_nn_attention_module",
            "torch_nn_transformer_block",
            "torch_nn_embedding_layer",
            "torch_autograd_backward",
            "torch_autograd_forward",
            "torch_tensor_cuda_memory_copy",
            "torch_tensor_cuda_kernel_launch",
            "torch_tensor_cuda_stream_synchronize",
            "torch_tensor_cuda_event_wait",
            "torch_tensor_cuda_device_synchronize",
            "nccl_collective_reduce",
            "nccl_collective_broadcast",
            "nccl_collective_all_gather",
            "nccl_comm_initialize",
            "nccl_comm_destroy",
            "nccl_group_start",
            "nccl_group_end",
            "nccl_reduce_scatter",
            "cuda_runtime_api_wrapper",
            "cuda_memory_pool_allocate",
            "cuda_memory_pool_free",
            "cuda_stream_create",
            "cuda_stream_destroy",
            "cuda_stream_synchronize",
            "cuda_memset_async",
            "cuda_memcpy_async",
            "thrust_sort",
            "thrust_reduce",
            "thrust_scan",
            "cub_device_scan",
            "cub_device_reduce",
            "mpi_allreduce",
            "mpi_bcast",
            "mpi_gather",
            "mpi_scatter",
            "mpi_send",
            "mpi_receive",
            "mpi_comm_rank",
            "mpi_comm_size",
        ];

        let suffixes = vec![
            "_kernel_launch",
            "_sync_operation",
            "_async_compute",
            "_memory_transfer",
            "_device_callback",
            "_host_init",
            "_device_finalize",
            "_stream_wait",
            "_event_synchronize",
            "_buffer_handle",
            "_resource_manager",
            "_cache_loader",
            "_gradient_accumulator",
            "_loss_calculator",
            "_activation_function",
            "_optimizer_step",
            "_parameter_gradient",
            "_weight_update",
            "_tensor_operations",
            "_matrix_multiplication",
            "_convolution_compute",
            "_attention_calculation",
            "_normalization",
            "_pooling_operation",
        ];

        let mut names = Vec::new();

        // 生成所有组合 - 产生大量长函数名
        for prefix in &prefixes {
            for suffix in &suffixes {
                names.push(format!("{}_{}", prefix, suffix));
                names.push(format!("_{}_impl", prefix));
                names.push(format!("{}_{}_v2", prefix, suffix));
                names.push(format!("torch_{}_internal", prefix));
            }
        }

        // 添加深度调用栈
        names.extend(vec![
            "main".to_string(),
            "train_epoch_full".to_string(),
            "train_step_forward_backward".to_string(),
            "model_forward_inference".to_string(),
            "model_backward_gradient".to_string(),
            "optimizer_step_parameters".to_string(),
            "cuda_runtime_wrapper_full".to_string(),
            "device_synchronize_full".to_string(),
            "nccl_all_reduce_ring_full".to_string(),
            "distributed_communication_full".to_string(),
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

    /// 生成单个 rank 的调用栈
    /// - max_depth >= 2000: 生成深栈（~100KB），用于性能测试 fixture
    /// - max_depth < 2000: 按 max_depth 生成浅栈，用于 mock server 等快速场景
    fn generate_single_stack(&self, rng: &mut impl Rng, rank: u32) -> String {
        let depth = if self.max_depth >= 2000 {
            rng.gen_range(2000..=2500)
        } else {
            let lo = (self.max_depth / 2).max(5);
            rng.gen_range(lo..=self.max_depth)
        };
        let mut stack = Vec::with_capacity(depth);

        // 添加共同的基础调用栈
        stack.push("main".to_string());
        stack.push("train_epoch_full".to_string());

        // 根据 rank 选择不同的路径
        match rank % 4 {
            0 => {
                stack.push("forward_pass_full".to_string());
                stack.push("model_inference_compute".to_string());
            }
            1 => {
                stack.push("backward_pass_full".to_string());
                stack.push("gradient_computation".to_string());
            }
            2 => {
                stack.push("optimizer_step_full".to_string());
                stack.push("parameter_update_calculation".to_string());
            }
            _ => {
                stack.push("sync_all_reduce_full".to_string());
                stack.push("distributed_communication".to_string());
            }
        }

        // 生成剩余的调用栈 - 使用更长的函数名
        while stack.len() < depth {
            let func = self.function_names.choose(rng).unwrap();

            // 添加重复模拟递归/循环调用
            if rng.gen_bool(0.3) && stack.len() > 10 {
                let idx = rng.gen_range(2..stack.len());
                let repeated_func = stack[idx].clone();
                stack.push(repeated_func);
            } else {
                stack.push(func.clone());
            }

            // 深层调用更可能是 CUDA/NCCL 相关
            if stack.len() > 50 && rng.gen_bool(0.6) {
                let cuda_funcs: Vec<_> = self
                    .function_names
                    .iter()
                    .filter(|f| f.contains("cuda") || f.contains("nccl") || f.contains("kernel"))
                    .collect();
                if !cuda_funcs.is_empty() {
                    stack.push(cuda_funcs.choose(rng).unwrap().to_string());
                }
            }
        }

        // 使用预分配容量提升性能
        // 100KB = 102400 字节，每层平均约 46 字符（39+1 分号）
        let mut result = String::with_capacity(depth * 46);
        for (i, frame) in stack.iter().enumerate() {
            if i > 0 {
                result.push(';');
            }
            result.push_str(frame);
        }
        result
    }

    /// 生成符合HTTP响应格式的火焰图数据
    pub fn generate_http_response_data(&self, rank_count: u32) -> Vec<String> {
        let flamegraph_data = self.generate_flamegraph_data(rank_count);

        flamegraph_data
            .iter()
            .map(|(rank, stack)| {
                // 模拟真实API响应格式
                format!("rank_{}: {}", rank, stack)
            })
            .collect()
    }

    /// 将 rank→stack 数据保存到 fixture 文件（每行一个 stack，按 rank 升序）
    pub fn save_fixture_data(stacks: &HashMap<u32, String>, path: &str) {
        if let Some(parent) = Path::new(path).parent() {
            std::fs::create_dir_all(parent).expect("create fixture dir");
        }
        let mut sorted: Vec<_> = stacks.iter().collect();
        sorted.sort_by_key(|(r, _)| *r);
        let content: String = sorted
            .iter()
            .map(|(_, s)| s.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(path, content).expect("write fixture file");
    }

    /// 从 fixture 文件加载 base stacks，并扩展到 total_ranks（rank N → base[N % base_count]）
    pub fn load_fixture_data(path: &str, total_ranks: u32) -> HashMap<u32, String> {
        let content = std::fs::read_to_string(path).expect("read fixture file");
        let base_stacks: Vec<&str> = content.split('\n').collect();
        let base_count = base_stacks.len() as u32;
        let mut data = HashMap::with_capacity(total_ranks as usize);
        for rank in 0..total_ranks {
            data.insert(rank, base_stacks[(rank % base_count) as usize].to_string());
        }
        data
    }

    /// fixture 路径是否存在
    pub fn fixture_exists(path: &str) -> bool {
        Path::new(path).exists()
    }

    /// 生成大规模测试数据集，用于压力测试
    pub fn generate_large_dataset(
        &self,
        total_ranks: u32,
        batch_size: u32,
    ) -> Vec<Vec<(u32, String)>> {
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

    /// 运行一次生成 fixture 文件（100 个 rank），之后测试直接加载它。
    /// 执行方式：cargo test generate_fixture_stacks --features "bench ssr" --lib -- --ignored
    #[test]
    #[ignore]
    fn generate_fixture_stacks() {
        let generator = FlameGraphDataGenerator::new(2500, 5); // 明确使用深栈（~100KB）
        let data = generator.generate_flamegraph_data(100);
        let path = "tests/fixtures/flamegraph_stacks.txt";
        FlameGraphDataGenerator::save_fixture_data(&data, path);
        println!("Saved {} base stacks to {}", data.len(), path);
        let total_size: usize = data.values().map(|s| s.len()).sum();
        println!(
            "Total fixture size: {:.2} MB",
            total_size as f64 / 1024.0 / 1024.0
        );
    }

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
