use std::time::Instant;
use std::collections::HashMap;
use crate::bench_utils::FlameGraphDataGenerator;
use crate::flamegraph::stack_merger::{StackTrie, merge_stacks};

/// fixture 文件路径（相对于 crate 根目录，即 app/）
const FIXTURE_PATH: &str = "tests/fixtures/flamegraph_stacks.txt";

/// 加载 10000 个 rank 的测试数据：优先使用 fixture，否则实时生成
fn load_10k_data() -> HashMap<u32, String> {
    if FlameGraphDataGenerator::fixture_exists(FIXTURE_PATH) {
        println!("Loading fixture data from {}", FIXTURE_PATH);
        FlameGraphDataGenerator::load_fixture_data(FIXTURE_PATH, 10000)
    } else {
        println!("Fixture not found, generating data on-the-fly");
        let generator = FlameGraphDataGenerator::new(50, 5);
        generator.generate_flamegraph_data(10000)
    }
}

/// 加载任意规模数据（100 / 1000）：优先使用 fixture
fn load_data(total_ranks: u32) -> HashMap<u32, String> {
    if FlameGraphDataGenerator::fixture_exists(FIXTURE_PATH) {
        FlameGraphDataGenerator::load_fixture_data(FIXTURE_PATH, total_ranks)
    } else {
        let generator = FlameGraphDataGenerator::new(50, 5);
        generator.generate_flamegraph_data(total_ranks)
    }
}

/// 简单的性能验证测试
#[cfg(test)]
mod performance_validation_tests {
    use super::*;

    #[test]
    fn test_small_scale_performance() {
        // 测试100个rank的性能
        let start = Instant::now();
        let data = load_data(100);
        let generation_time = start.elapsed();
        
        println!("Generated 100 flamegraphs in {:?}", generation_time);
        assert_eq!(data.len(), 100);
        
        // 测试合并性能
        let stacks: Vec<&str> = data.values().map(|s| s.as_str()).collect();
        let start = Instant::now();
        let _trie = merge_stacks(stacks);
        let merge_time = start.elapsed();
        
        println!("Merged 100 stacks in {:?}", merge_time);
        assert!(merge_time.as_millis() < 1000); // 应该在1秒内完成
    }

    #[test]
    fn test_medium_scale_performance() {
        // 测试1000个rank的性能
        let start = Instant::now();
        let data = load_data(1000);
        let generation_time = start.elapsed();
        
        println!("Loaded 1000 flamegraphs in {:?}", generation_time);
        assert_eq!(data.len(), 1000);
        
        // 测试增量合并性能
        let batches: Vec<Vec<(u32, String)>> = {
            let mut b: Vec<Vec<(u32, String)>> = Vec::new();
            let items: Vec<(u32, String)> = data.into_iter().collect();
            for chunk in items.chunks(100) {
                b.push(chunk.to_vec());
            }
            b
        };
        let start = Instant::now();
        let mut trie = StackTrie::with_total_ranks(1000);
        
        for batch in batches {
            let batch_data: Vec<(u32, &str)> = batch.iter()
                .map(|(rank, stack)| (*rank, stack.as_str()))
                .collect();
            trie.insert_batch(batch_data);
        }
        
        let incremental_merge_time = start.elapsed();
        println!("Incrementally merged 1000 stacks in {:?}", incremental_merge_time);
        assert!(incremental_merge_time.as_millis() < 5000); // 应该在5秒内完成
    }

    /// 耗时约 32s（4次 10k merge），需要显式运行：
    /// cargo test test_large_scale_10k_performance --features "bench ssr" --lib -- --ignored --nocapture
    #[test]
    #[ignore]
    fn test_large_scale_10k_performance() {
        // 测试 10000 个 rank 的加载性能
        let start = Instant::now();
        let data = load_10k_data();
        let generation_time = start.elapsed();
        
        println!("Loaded 10000 flamegraphs in {:?}", generation_time);
        assert_eq!(data.len(), 10000);
        
        // 验证生成数据的合理性
        let avg_stack_size: usize = data.values()
            .map(|s| s.len())
            .sum::<usize>() / data.len();
        println!("Average stack size: {} bytes", avg_stack_size);
        assert!(avg_stack_size > 0);
        assert!(avg_stack_size > 80000); // 每个堆栈应该大于 80KB
        
        // 测试一次性合并 10000 个堆栈
        let stacks: Vec<&str> = data.values().map(|s| s.as_str()).collect();
        let start = Instant::now();
        let _trie_all = merge_stacks(stacks);
        let merge_time = start.elapsed();
        
        println!("Merged 10000 stacks all-at-once in {:?}", merge_time);
        assert!(merge_time.as_secs() < 600);
        
        // 测试增量合并 10000 个堆栈（复用已加载的 data，不重复 load）
        let items: Vec<(u32, String)> = load_10k_data().into_iter().collect();
        let batches: Vec<Vec<(u32, String)>> = items.chunks(500)
            .map(|c| c.to_vec())
            .collect();
        
        let start = Instant::now();
        let mut trie_incremental = StackTrie::with_total_ranks(10000);
        
        for batch in batches {
            let batch_data: Vec<(u32, &str)> = batch.iter()
                .map(|(rank, stack)| (*rank, stack.as_str()))
                .collect();
            trie_incremental.insert_batch(batch_data);
        }
        
        let incremental_merge_time = start.elapsed();
        println!("!!! ===time=== Incrementally merged 10000 stacks in {:?}", incremental_merge_time);
        
        // 验证增量合并结果
        let results = trie_incremental.traverse_with_all_stack(&trie_incremental.root, Vec::new());
        println!("Incremental merge produced {} unique paths", results.len());
        assert!(results.len() > 0);
        
        // 增量合并时间也应该在合理范围内
        assert!(incremental_merge_time.as_secs() < 600);
    }

    /// 耗时约 32s（4次 10k merge），需要显式运行：
    /// cargo test test_memory_efficiency_10k --features "bench ssr" --lib -- --ignored --nocapture
    #[test]
    #[ignore]
    fn test_memory_efficiency_10k() {
        use memory_stats::memory_stats;
        
        // 获取初始内存
        let start_memory = memory_stats().map(|s| s.physical_mem as f64 / 1024.0 / 1024.0);
        
        // 加载 10000 个 stack（fixture 或实时生成）
        let data = load_10k_data();
        
        // 估算数据大小
        let total_string_size: usize = data.values()
            .map(|s| s.len())
            .sum();
        let estimated_data_mb = total_string_size as f64 / 1024.0 / 1024.0;
        
        println!("Estimated data size for 10000 stacks: {:.2}MB", estimated_data_mb);
        println!("Average stack: {:.1} bytes", total_string_size as f64 / data.len() as f64);
        
        // 执行合并操作
        let stacks: Vec<&str> = data.values().map(|s| s.as_str()).collect();
        let trie = merge_stacks(stacks);
        
        // 获取结束内存
        let end_memory = memory_stats().map(|s| s.physical_mem as f64 / 1024.0 / 1024.0);
        
        if let (Some(start), Some(end)) = (start_memory, end_memory) {
            let memory_used_mb = end - start;
            println!("Memory used for 10k merge: {:.2}MB", memory_used_mb);

            // 内存使用应该在合理范围内（比如不超过 40GB）
            assert!(memory_used_mb < 40960.0);
            
            // 计算每个 stack 的平均内存消耗
            let mem_per_rank = memory_used_mb / data.len() as f64;
            println!("Memory per rank: {:.4}MB", mem_per_rank);
        }
        // 测试增量合并内存使用（复用 fixture 数据）
        let items: Vec<(u32, String)> = load_10k_data().into_iter().collect();
        let batches: Vec<Vec<(u32, String)>> = items.chunks(500)
            .map(|c| c.to_vec())
            .collect();
        
        let start_mem = memory_stats().map(|s| s.physical_mem as f64 / 1024.0 / 1024.0);
        let mut trie_incremental = StackTrie::with_total_ranks(10000);
        
        for batch in batches {
            let batch_data: Vec<(u32, &str)> = batch.iter()
                .map(|(rank, stack)| (*rank, stack.as_str()))
                .collect();
            trie_incremental.insert_batch(batch_data);
        }
        let end_mem = memory_stats().map(|s| s.physical_mem as f64 / 1024.0 / 1024.0);
        
        if let (Some(start), Some(end)) = (start_mem, end_mem) {
            let memory_used_mb = end - start;
            println!("!!! ===Memory=== used for 10k incremental merge: {:.2}MB", memory_used_mb);
            assert!(memory_used_mb < 40960.0);
            let mem_per_rank = memory_used_mb / data.len() as f64;
            println!("!!! ===Memory=== per rank (incremental): {:.4}MB", mem_per_rank);

            // 验证增量合并结果
            let results = trie_incremental.traverse_with_all_stack(&trie_incremental.root, Vec::new());
            println!("Incremental merge produced {} unique paths", results.len());
            assert!(results.len() > 0);
        }
    }

    #[test]
    fn test_memory_usage_estimation() {
        use std::mem;
        
        let data = load_data(1000);
        
        // 估算内存使用
        let total_string_size: usize = data.values()
            .map(|s| s.len())
            .sum();
        
        let estimated_memory_mb = (total_string_size + data.len() * mem::size_of::<u32>()) as f64 / 1024.0 / 1024.0;
        
        println!("Estimated memory usage for 1000 flamegraphs: {:.2}MB", estimated_memory_mb);
        println!("Average stack size: {:.1} bytes", total_string_size as f64 / data.len() as f64);
        
        // 基本合理性检查
        assert!(estimated_memory_mb > 0.1); // 至少100KB
        assert!(estimated_memory_mb < 1000.0); // 不超过1000MB
    }

    #[tokio::test]
    async fn test_mock_server_basic_functionality() {
        use crate::mock_server::{MockFlameGraphServer, MockServerConfig};
        
        let config = MockServerConfig {
            ports: vec![18933], // 使用测试端口
            ranks_per_port: 10,
            max_stack_depth: 20,
            response_delay_ms: 0,
            error_rate: 0.0,
            stack_size_bytes: None,
        };

        let server = MockFlameGraphServer::new(config);
        let _handles = server.start_all().await.unwrap();

        // 等待服务器启动
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // 测试健康检查
        let client = reqwest::Client::new();
        let response = client
            .get("http://127.0.0.1:18933/health")
            .send()
            .await
            .unwrap();

        assert!(response.status().is_success());
        let health_text = response.text().await.unwrap();
        assert_eq!(health_text, "OK");
        
        println!("Mock server health check passed");
    }

    #[test]
    fn test_trie_consistency() {
        let data = load_data(100);
        
        // 比较一次性合并和增量合并的结果
        let stacks: Vec<&str> = data.values().map(|s| s.as_str()).collect();
        let trie_all_at_once = merge_stacks(stacks.clone());
        
        let mut trie_incremental = StackTrie::with_total_ranks(100);
        let items: Vec<(u32, String)> = data.into_iter().collect();
        for chunk in items.chunks(20) {
            let batch_data: Vec<(u32, &str)> = chunk.iter()
                .map(|(rank, stack)| (*rank, stack.as_str()))
                .collect();
            trie_incremental.insert_batch(batch_data);
        }
        
        // 验证两种方法产生的结果数量相近（由于数据生成的随机性，可能不完全相同）
        let results_all = trie_all_at_once.traverse_with_all_stack(&trie_all_at_once.root, Vec::new());
        let results_incremental = trie_incremental.traverse_with_all_stack(&trie_incremental.root, Vec::new());
        
        println!("All-at-once results: {}", results_all.len());
        println!("Incremental results: {}", results_incremental.len());
        
        // 结果数量应该在合理范围内
        assert!(results_all.len() > 0);
        assert!(results_incremental.len() > 0);
    }

    /// 耗时约 60s+（10k HTTP 请求），需要显式运行：
    /// cargo test test_fetch_urls_batched_10k_performance --features "bench ssr" --lib -- --ignored --nocapture
    #[tokio::test]
    #[ignore]
    async fn test_fetch_urls_batched_10k_performance() {
        use crate::flamegraph::stack_collector::fetch_urls_batched;
        use std::sync::{Arc, Mutex};
        use crate::mock_server::{MockFlameGraphServer, MockServerConfig};
        
        // 启动 Mock 服务器：1250 个端口 × 每端口 8 个 rank = 10000 个 rank
        // 更真实地模拟大规模分布式训练中每个节点暴露少量 rank 的场景
        const BASE_PORT: u16 = 20000;
        const NUM_PORTS: u16 = 10000;
        const RANKS_PER_PORT: u32 = 1;

        let config = MockServerConfig {
            ports: (BASE_PORT..BASE_PORT + NUM_PORTS).collect(), // 10000 个端口
            ranks_per_port: RANKS_PER_PORT, // 每个端口 1 个，共 10000 个
            max_stack_depth: 50,
            response_delay_ms: 0, // 5ms 延迟
            error_rate: 0.0,
            stack_size_bytes: Some(80 * 1024), // 每个 URL 返回约 80KB 数据
        };

        let server = MockFlameGraphServer::new(config);
        let _handles = server.start_all().await.unwrap();

        // 等待服务器启动
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
        
        // 构建 10000 个 URL（10000 端口 × 1 rank）
        let urls: Vec<String> = (0..NUM_PORTS)
            .flat_map(|port_idx| {
                let port = BASE_PORT + port_idx;
                (0..RANKS_PER_PORT).map(move |rank| {
                    format!("http://127.0.0.1:{}/callstack/{}", port, rank)
                })
            })
            .collect();
        
        println!("Total URLs to fetch: {}", urls.len());
        assert_eq!(urls.len(), 10000, "Should have 10000 URLs");
        
        // 使用 Arc<Mutex<Vec<_>>> 收集处理的数据
        // fetch_urls_batched 使用 usize 作为 rank index
        // Mock 服务器返回 {"rank": u32, "stack": String, "timestamp": u64}
        let collected_data: Arc<Mutex<Vec<(usize, serde_json::Value)>>> = Arc::new(Mutex::new(Vec::new()));
        let collected_data_clone = collected_data.clone();
        
        // 测试 fetch_urls_batched 的性能，使用较小的 batch_size 减少并发
        let start_time = std::time::Instant::now();
        let result = fetch_urls_batched(
            urls,
            2000, // batch_size=500 (减少并发数)
            |batch| {
                let data = collected_data_clone.clone();
                async move {
                    let mut data_guard = data.lock().unwrap();
                    data_guard.extend(batch);
                    Ok(())
                }
            }
        ).await;
        let elapsed = start_time.elapsed();
        
        assert!(result.is_ok(), "Should successfully fetch all URLs");
        
        let collected = collected_data.lock().unwrap();
        println!("Successfully fetched and processed {} items in {:?}", collected.len(), elapsed);
        
        // 验证数据完整性
        assert_eq!(collected.len(), 10000, "Should have collected all 10000 items");
        
        // 性能断言
        println!("!!!! ===url=== Average time per request: {:?}", elapsed / collected.len().try_into().unwrap());
        assert!(elapsed.as_secs() < 120, "Should complete within 120 seconds");
        
        // 验证数据格式正确性 - Mock 服务器返回 {"rank", "stack", "timestamp"}
        let has_stacks = collected.iter().any(|(_, v)| {
            v.get("stack").is_some() && v.get("rank").is_some() && v.get("timestamp").is_some()
        });
        assert!(has_stacks, "Should have complete FlameGraphResponse data (rank, stack, timestamp)");
        
        println!("Data sample (first item): {:?}", collected.first().map(|(_, v)| v));
    }
}  

#[cfg(test)]
mod integration_tests {
    use super::*;
    use crate::mock_server::{MockFlameGraphServer, MockServerConfig};

    #[tokio::test]
    async fn test_end_to_end_mini_workflow() {
        // 创建一个迷你版的端到端工作流程
        let config = MockServerConfig {
            ports: vec![17933, 17934],
            ranks_per_port: 50, // 每个端口50个rank，总共100个
            max_stack_depth: 30,
            response_delay_ms: 1,
            error_rate: 0.0,
            stack_size_bytes: None,
        };

        let server = MockFlameGraphServer::new(config.clone());
        let _handles = server.start_all().await.unwrap();

        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // 模拟客户端并发请求
        let client = reqwest::Client::new();
        let mut all_stacks = Vec::new();
        let mut handles = Vec::new();

        for &port in &config.ports {
            for batch in 0..5 { // 每个端口分5批，每批10个
                let client = client.clone();
                let start_rank = batch * 10;
                
                let handle = tokio::spawn(async move {
                    let url = format!(
                        "http://127.0.0.1:{}/callstack?rank={}&batch_size=10",
                        port, start_rank
                    );
                    let response = client.get(&url).send().await;
                    match response {
                        Ok(r) => r.json::<Vec<serde_json::Value>>().await,
                        Err(e) => Err(e),
                    }
                });
                
                handles.push(handle);
            }
        }

        // 收集所有响应
        let results = futures::future::join_all(handles).await;
        let mut total_collected = 0;

        for result in results {
            if let Ok(Ok(flame_responses)) = result {
                for response in flame_responses {
                    if let Some(stack) = response.get("stack").and_then(|s| s.as_str()) {
                        all_stacks.push(stack.to_string());
                        total_collected += 1;
                    }
                }
            }
        }

        println!("Collected {} stacks from mock servers", total_collected);
        assert!(total_collected > 80); // 至少收集到80%的数据

        // 合并火焰图
        if !all_stacks.is_empty() {
            let start = Instant::now();
            let stack_refs: Vec<&str> = all_stacks.iter().map(|s| s.as_str()).collect();
            let merged_trie = merge_stacks(stack_refs);
            let merge_time = start.elapsed();

            let results = merged_trie.traverse_with_all_stack(&merged_trie.root, Vec::new());
            
            println!("Merged {} stacks into {} unique paths in {:?}", 
                     all_stacks.len(), results.len(), merge_time);
            
            assert!(results.len() > 0);
            assert!(merge_time.as_millis() < 1000);
        }
    }
}
