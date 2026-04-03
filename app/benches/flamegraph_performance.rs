use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use memory_stats::memory_stats;
use std::time::Duration;

// Import the modules we'll be testing
use app::bench_utils::FlameGraphDataGenerator;
use app::flamegraph::stack_merger::{StackTrie, merge_stacks};

/// 内存使用情况统计
#[derive(Debug, Clone)]
pub struct MemoryUsage {
    pub physical_mem_mb: f64,
    pub virtual_mem_mb: f64,
}

impl MemoryUsage {
    pub fn current() -> Option<Self> {
        memory_stats().map(|stats| Self {
            physical_mem_mb: stats.physical_mem as f64 / 1024.0 / 1024.0,
            virtual_mem_mb: stats.virtual_mem as f64 / 1024.0 / 1024.0,
        })
    }
}

/// 性能测试配置
struct BenchmarkConfig {
    rank_counts: Vec<u32>,
    max_stack_depth: usize,
    batch_sizes: Vec<u32>,
}

impl Default for BenchmarkConfig {
    fn default() -> Self {
        Self {
            rank_counts: vec![100, 500, 1000, 5000, 10000],
            max_stack_depth: 50,
            batch_sizes: vec![100, 500, 1000],
        }
    }
}

/// 基准测试：火焰图数据生成性能
fn bench_flamegraph_generation(c: &mut Criterion) {
    let config = BenchmarkConfig::default();
    let mut group = c.benchmark_group("flamegraph_generation");

    for &rank_count in &config.rank_counts {
        group.throughput(Throughput::Elements(rank_count as u64));
        group.bench_with_input(
            BenchmarkId::new("generate_data", rank_count),
            &rank_count,
            |b, &rank_count| {
                let generator = FlameGraphDataGenerator::new(config.max_stack_depth, 5);
                
                b.iter_custom(|iters| {
                    let start_memory = MemoryUsage::current();
                    let start_time = std::time::Instant::now();
                    
                    for _ in 0..iters {
                        let _data = black_box(generator.generate_flamegraph_data(rank_count));
                    }
                    
                    let elapsed = start_time.elapsed();
                    let end_memory = MemoryUsage::current();
                    
                    // 打印内存使用情况
                    if let (Some(start), Some(end)) = (start_memory, end_memory) {
                        println!(
                            "Memory for {} ranks: {:.2}MB -> {:.2}MB (diff: {:.2}MB)",
                            rank_count,
                            start.physical_mem_mb,
                            end.physical_mem_mb,
                            end.physical_mem_mb - start.physical_mem_mb
                        );
                    }
                    
                    elapsed
                });
            },
        );
    }
    group.finish();
}

/// 基准测试：火焰图合并性能
fn bench_flamegraph_merging(c: &mut Criterion) {
    let config = BenchmarkConfig::default();
    let mut group = c.benchmark_group("flamegraph_merging");

    for &rank_count in &config.rank_counts {
        group.throughput(Throughput::Elements(rank_count as u64));
        group.measurement_time(Duration::from_secs(30));
        
        // 预生成测试数据
        let generator = FlameGraphDataGenerator::new(config.max_stack_depth, 5);
        let test_data = generator.generate_flamegraph_data(rank_count);
        let stacks: Vec<&str> = test_data.values().map(|s| s.as_str()).collect();
        
        group.bench_with_input(
            BenchmarkId::new("merge_all_at_once", rank_count),
            &stacks,
            |b, stacks| {
                b.iter_custom(|iters| {
                    let start_memory = MemoryUsage::current();
                    let start_time = std::time::Instant::now();
                    
                    for _ in 0..iters {
                        let _trie = black_box(merge_stacks(stacks.clone()));
                    }
                    
                    let elapsed = start_time.elapsed();
                    let end_memory = MemoryUsage::current();
                    
                    if let (Some(start), Some(end)) = (start_memory, end_memory) {
                        println!(
                            "Memory for merging {} stacks: {:.2}MB -> {:.2}MB (diff: {:.2}MB)",
                            stacks.len(),
                            start.physical_mem_mb,
                            end.physical_mem_mb,
                            end.physical_mem_mb - start.physical_mem_mb
                        );
                    }
                    
                    elapsed
                });
            },
        );
    }
    group.finish();
}

/// 基准测试：增量合并性能
fn bench_incremental_merging(c: &mut Criterion) {
    let config = BenchmarkConfig::default();
    let mut group = c.benchmark_group("incremental_merging");

    for &rank_count in &config.rank_counts {
        group.throughput(Throughput::Elements(rank_count as u64));
        
        for &batch_size in &config.batch_sizes {
            if batch_size >= rank_count {
                continue;
            }
            
            let generator = FlameGraphDataGenerator::new(config.max_stack_depth, 5);
            let batches = generator.generate_large_dataset(rank_count, batch_size);
            
            group.bench_with_input(
                BenchmarkId::new(
                    format!("incremental_{}ranks_{}batch", rank_count, batch_size),
                    rank_count
                ),
                &batches,
                |b, batches| {
                    b.iter_custom(|iters| {
                        let start_memory = MemoryUsage::current();
                        let start_time = std::time::Instant::now();
                        
                        for _ in 0..iters {
                            let mut trie = StackTrie::with_total_ranks(rank_count);
                            
                            for batch in batches {
                                let batch_data: Vec<(u32, &str)> = batch.iter()
                                    .map(|(rank, stack)| (*rank, stack.as_str()))
                                    .collect();
                                trie.insert_batch(batch_data);
                            }
                            
                            black_box(trie);
                        }
                        
                        let elapsed = start_time.elapsed();
                        let end_memory = MemoryUsage::current();
                        
                        if let (Some(start), Some(end)) = (start_memory, end_memory) {
                            println!(
                                "Memory for incremental {} ranks (batch {}): {:.2}MB -> {:.2}MB (diff: {:.2}MB)",
                                rank_count,
                                batch_size,
                                start.physical_mem_mb,
                                end.physical_mem_mb,
                                end.physical_mem_mb - start.physical_mem_mb
                            );
                        }
                        
                        elapsed
                    });
                },
            );
        }
    }
    group.finish();
}

/// 基准测试：数据结构大小和内存效率
fn bench_memory_efficiency(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory_efficiency");
    let config = BenchmarkConfig::default();
    
    // 测试不同规模下的内存效率
    for &rank_count in &config.rank_counts {
        group.bench_with_input(
            BenchmarkId::new("trie_memory_usage", rank_count),
            &rank_count,
            |b, &rank_count| {
                let generator = FlameGraphDataGenerator::new(config.max_stack_depth, 5);
                let data = generator.generate_flamegraph_data(rank_count);
                let stacks: Vec<&str> = data.values().map(|s| s.as_str()).collect();
                
                b.iter(|| {
                    let start_mem = MemoryUsage::current();
                    let trie = black_box(merge_stacks(stacks.clone()));
                    let results = trie.traverse_with_all_stack(&trie.root, Vec::new());
                    let end_mem = MemoryUsage::current();
                    
                    if let (Some(start), Some(end)) = (start_mem, end_mem) {
                        let mem_per_rank = (end.physical_mem_mb - start.physical_mem_mb) / rank_count as f64;
                        println!("Memory per rank for {}: {:.4}MB, unique paths: {}", 
                                rank_count, mem_per_rank, results.len());
                    }
                    
                    black_box((results.len(), trie))
                });
            },
        );
    }
    group.finish();
}

/// 基准测试：万卡级别综合测试
fn bench_10k_comprehensive(c: &mut Criterion) {
    let mut group = c.benchmark_group("comprehensive_10k");
    group.measurement_time(Duration::from_secs(60));
    group.sample_size(10); // 减少样本数量以节省时间
    
    let generator = FlameGraphDataGenerator::new(50, 5);
    
    group.bench_function("generate_10k_stacks", |b| {
        b.iter_custom(|iters| {
            let start_memory = MemoryUsage::current();
            let start_time = std::time::Instant::now();
            
            for _ in 0..iters {
                let _data = black_box(generator.generate_flamegraph_data(10000));
            }
            
            let elapsed = start_time.elapsed();
            let end_memory = MemoryUsage::current();
            
            if let (Some(start), Some(end)) = (start_memory, end_memory) {
                println!(
                    "10K generation: {:.2}MB -> {:.2}MB (diff: {:.2}MB)",
                    start.physical_mem_mb,
                    end.physical_mem_mb,
                    end.physical_mem_mb - start.physical_mem_mb
                );
            }
            
            elapsed
        });
    });
    
    group.bench_function("merge_10k_stacks_incremental", |b| {
        let batches = generator.generate_large_dataset(10000, 500);
        
        b.iter_custom(|iters| {
            let start_memory = MemoryUsage::current();
            let start_time = std::time::Instant::now();
            
            for _ in 0..iters {
                let mut trie = StackTrie::with_total_ranks(10000);
                
                for batch in &batches {
                    let batch_data: Vec<(u32, &str)> = batch.iter()
                        .map(|(rank, stack)| (*rank, stack.as_str()))
                        .collect();
                    trie.insert_batch(batch_data);
                }
                
                let results = trie.traverse_with_all_stack(&trie.root, Vec::new());
                black_box((trie, results.len()));
            }
            
            let elapsed = start_time.elapsed();
            let end_memory = MemoryUsage::current();
            
            if let (Some(start), Some(end)) = (start_memory, end_memory) {
                println!(
                    "10K incremental merge: {:.2}MB -> {:.2}MB (diff: {:.2}MB)",
                    start.physical_mem_mb,
                    end.physical_mem_mb,
                    end.physical_mem_mb - start.physical_mem_mb
                );
            }
            
            elapsed
        });
    });
    
    group.finish();
}

criterion_group!(
    benches,
    bench_flamegraph_generation,
    bench_flamegraph_merging,
    bench_incremental_merging,
    bench_memory_efficiency,
    bench_10k_comprehensive
);
criterion_main!(benches);