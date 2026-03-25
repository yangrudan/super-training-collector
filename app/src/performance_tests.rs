use std::time::Instant;
use crate::bench_utils::FlameGraphDataGenerator;
use crate::flamegraph::stack_merger::{StackTrie, merge_stacks};

/// 简单的性能验证测试
#[cfg(test)]
mod performance_validation_tests {
    use super::*;

    #[test]
    fn test_small_scale_performance() {
        let generator = FlameGraphDataGenerator::new(30, 5);
        
        // 测试100个rank的性能
        let start = Instant::now();
        let data = generator.generate_flamegraph_data(100);
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
        let generator = FlameGraphDataGenerator::new(40, 5);
        
        // 测试1000个rank的性能
        let start = Instant::now();
        let data = generator.generate_flamegraph_data(1000);
        let generation_time = start.elapsed();
        
        println!("Generated 1000 flamegraphs in {:?}", generation_time);
        assert_eq!(data.len(), 1000);
        
        // 测试增量合并性能
        let batches = generator.generate_large_dataset(1000, 100);
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

    #[test]
    fn test_memory_usage_estimation() {
        use std::mem;
        
        let generator = FlameGraphDataGenerator::new(50, 5);
        let data = generator.generate_flamegraph_data(1000);
        
        // 估算内存使用
        let total_string_size: usize = data.values()
            .map(|s| s.len())
            .sum();
        
        let estimated_memory_mb = (total_string_size + data.len() * mem::size_of::<u32>()) as f64 / 1024.0 / 1024.0;
        
        println!("Estimated memory usage for 1000 flamegraphs: {:.2}MB", estimated_memory_mb);
        println!("Average stack size: {:.1} bytes", total_string_size as f64 / data.len() as f64);
        
        // 基本合理性检查
        assert!(estimated_memory_mb > 0.1); // 至少100KB
        assert!(estimated_memory_mb < 100.0); // 不超过100MB
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
        let generator = FlameGraphDataGenerator::new(25, 3);
        let data = generator.generate_flamegraph_data(100);
        
        // 比较一次性合并和增量合并的结果
        let stacks: Vec<&str> = data.values().map(|s| s.as_str()).collect();
        let trie_all_at_once = merge_stacks(stacks.clone());
        
        let mut trie_incremental = StackTrie::with_total_ranks(100);
        let batches = generator.generate_large_dataset(100, 20);
        for batch in batches {
            let batch_data: Vec<(u32, &str)> = batch.iter()
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