use axum::{
    extract::{Query, Path},
    http::StatusCode,
    response::Json,
    routing::get,
    Router,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::RwLock;
use tower_http::cors::CorsLayer;

use app::bench_utils::FlameGraphDataGenerator;

/// Mock服务器配置
#[derive(Debug, Clone)]
pub struct MockServerConfig {
    pub ports: Vec<u16>,
    pub ranks_per_port: u32,
    pub max_stack_depth: usize,
    pub response_delay_ms: u64,
    pub error_rate: f64,
    /// 若设置，则每个 callstack 响应的 stack 字段大小约为此字节数，
    /// 使用按需生成（不预计算），适合大数据量性能测试。
    pub stack_size_bytes: Option<usize>,
}

impl Default for MockServerConfig {
    fn default() -> Self {
        Self {
            ports: vec![9933, 9934, 9935, 9936],
            ranks_per_port: 2500, // 总共10000 ranks分布在4个端口上
            max_stack_depth: 50,
            response_delay_ms: 10,
            error_rate: 0.01, // 1%错误率
            stack_size_bytes: None,
        }
    }
}

/// API查询参数
#[derive(Deserialize)]
struct CallstackQuery {
    rank: Option<u32>,
    batch_size: Option<u32>,
}

/// 火焰图响应数据
#[derive(Serialize, Deserialize)]
struct FlameGraphResponse {
    rank: u32,
    stack: String,
    timestamp: u64,
}

/// Mock服务器状态
pub struct MockServerState {
    generator: FlameGraphDataGenerator,
    flamegraph_data: HashMap<u32, String>,
    config: MockServerConfig,
}

impl MockServerState {
    fn new(config: MockServerConfig) -> Self {
        let generator = FlameGraphDataGenerator::new(config.max_stack_depth, 5);
        let total_ranks = config.ports.len() as u32 * config.ranks_per_port;
        let flamegraph_data = generator.generate_flamegraph_data(total_ranks);
        
        Self {
            generator,
            flamegraph_data,
            config,
        }
    }
}

/// Mock HTTP服务器，模拟火焰图数据API
pub struct MockFlameGraphServer {
    config: MockServerConfig,
    state: Arc<RwLock<MockServerState>>,
}

impl MockFlameGraphServer {
    pub fn new(config: MockServerConfig) -> Self {
        let state = Arc::new(RwLock::new(MockServerState::new(config.clone())));
        Self { config, state }
    }

    /// 启动所有端口的Mock服务器
    pub async fn start_all(&self) -> Result<Vec<tokio::task::JoinHandle<()>>, Box<dyn std::error::Error>> {
        let mut handles = Vec::new();
        
        for (port_idx, &port) in self.config.ports.iter().enumerate() {
            let state = Arc::clone(&self.state);
            let config = self.config.clone();
            
            let handle = tokio::spawn(async move {
                if let Err(e) = Self::start_single_server(port, port_idx, state, config).await {
                    eprintln!("Mock server on port {} failed: {}", port, e);
                }
            });
            
            handles.push(handle);
        }
        
        // 等待所有服务器启动
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        println!("Mock servers started on ports: {:?}", self.config.ports);
        
        Ok(handles)
    }

    /// 启动单个端口的服务器
    async fn start_single_server(
        port: u16, 
        port_idx: usize, 
        state: Arc<RwLock<MockServerState>>,
        config: MockServerConfig
    ) -> Result<(), Box<dyn std::error::Error>> {
        let app = Router::new()
            .route("/callstack/{rank}", get(get_single_callstack))
            .route("/callstack", get(get_batch_callstack))
            .route("/health", get(health_check))
            .layer(CorsLayer::permissive())
            .with_state((state, port_idx, config));

        let addr = SocketAddr::from(([0, 0, 0, 0], port));
        let listener = tokio::net::TcpListener::bind(addr).await?;
        
        println!("Mock server listening on port {}", port);
        axum::serve(listener, app).await?;
        Ok(())
    }
}

/// 按需生成指定字节大小的调用栈字符串（确定性，基于 rank 做偏移）
fn generate_stack_for_rank(rank: u32, target_bytes: usize) -> String {
    const FUNC_NAMES: &[&str] = &[
        "cuda_nccl_communicator_kernel_launch",
        "torch_distributed_allreduce_sync_operation",
        "torch_distributed_allgather_async_compute",
        "torch_distributed_broadcast_memory_transfer",
        "torch_distributed_reduce_scatter_device_callback",
        "torch_tensor_forward_pass_host_init",
        "torch_tensor_backward_pass_device_finalize",
        "torch_optimizer_parameter_update_stream_wait",
        "torch_nn_linear_layer_event_synchronize",
        "torch_nn_conv2d_layer_buffer_handle",
        "torch_nn_batch_norm_resource_manager",
        "torch_nn_dropout_cache_loader",
        "torch_nn_attention_module_gradient_accumulator",
        "torch_nn_transformer_block_loss_calculator",
        "torch_nn_embedding_layer_activation_function",
        "torch_autograd_backward_optimizer_step",
        "torch_autograd_forward_parameter_gradient",
        "torch_tensor_cuda_memory_copy_weight_update",
        "torch_tensor_cuda_kernel_launch_tensor_operations",
        "torch_tensor_cuda_stream_synchronize_matrix_multiplication",
        "nccl_collective_reduce_convolution_compute",
        "nccl_collective_broadcast_attention_calculation",
        "nccl_collective_all_gather_normalization",
        "nccl_comm_initialize_pooling_operation",
        "cuda_runtime_api_wrapper_async_compute",
        "cuda_memory_pool_allocate_memory_transfer",
        "cuda_memory_pool_free_device_callback",
        "cuda_stream_create_host_init",
        "cuda_stream_synchronize_device_finalize",
        "mpi_allreduce_stream_wait",
        "mpi_bcast_event_synchronize",
        "mpi_gather_buffer_handle",
        "mpi_scatter_resource_manager",
    ];

    let mut result = String::with_capacity(target_bytes + 64);
    result.push_str("main;train_epoch_full");

    let mut i = 0usize;
    while result.len() < target_bytes {
        result.push(';');
        let name = FUNC_NAMES[(rank as usize * 7 + i * 13) % FUNC_NAMES.len()];
        result.push_str(name);
        i += 1;
    }
    result
}

/// 获取单个rank的调用栈
async fn get_single_callstack(
    Path(rank): Path<u32>,
    axum::extract::State((state, port_idx, config)): axum::extract::State<(
        Arc<RwLock<MockServerState>>, 
        usize, 
        MockServerConfig
    )>
) -> Result<Json<FlameGraphResponse>, StatusCode> {
    // 模拟延迟
    // if config.response_delay_ms > 0 {
    //     tokio::time::sleep(tokio::time::Duration::from_millis(config.response_delay_ms)).await;
    // }

    // 模拟错误
    if rand::random::<f64>() < config.error_rate {
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }

    let base_rank = port_idx as u32 * config.ranks_per_port;
    let actual_rank = base_rank + rank;

    // 若配置了目标字节大小，则按需生成，无需预计算数据
    if let Some(target_bytes) = config.stack_size_bytes {
        let stack = generate_stack_for_rank(actual_rank, target_bytes);
        return Ok(Json(FlameGraphResponse {
            rank: actual_rank,
            stack,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        }));
    }

    let state_guard = state.read().await;
    if let Some(stack) = state_guard.flamegraph_data.get(&actual_rank) {
        Ok(Json(FlameGraphResponse {
            rank: actual_rank,
            stack: stack.clone(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        }))
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

/// 批量获取调用栈
async fn get_batch_callstack(
    Query(params): Query<CallstackQuery>,
    axum::extract::State((state, port_idx, config)): axum::extract::State<(
        Arc<RwLock<MockServerState>>, 
        usize, 
        MockServerConfig
    )>
) -> Result<Json<Vec<FlameGraphResponse>>, StatusCode> {
    // 模拟延迟
    // if config.response_delay_ms > 0 {
    //     tokio::time::sleep(tokio::time::Duration::from_millis(config.response_delay_ms)).await;
    // }

    // 模拟错误
    if rand::random::<f64>() < config.error_rate {
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }

    let state_guard = state.read().await;
    let batch_size = params.batch_size.unwrap_or(100);
    let start_rank = params.rank.unwrap_or(0);
    
    let base_rank = port_idx as u32 * config.ranks_per_port;
    let mut responses = Vec::new();
    
    for i in 0..batch_size {
        let actual_rank = base_rank + start_rank + i;
        
        if let Some(stack) = state_guard.flamegraph_data.get(&actual_rank) {
            responses.push(FlameGraphResponse {
                rank: actual_rank,
                stack: stack.clone(),
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
            });
        }
    }
    
    Ok(Json(responses))
}

/// 健康检查端点
async fn health_check() -> &'static str {
    "OK"
}

/// 用于性能测试的Mock服务器管理器
pub struct MockServerManager {
    servers: Vec<MockFlameGraphServer>,
    handles: Vec<tokio::task::JoinHandle<()>>,
}

impl MockServerManager {
    pub fn new(configs: Vec<MockServerConfig>) -> Self {
        let servers = configs.into_iter()
            .map(MockFlameGraphServer::new)
            .collect();
        
        Self {
            servers,
            handles: Vec::new(),
        }
    }

    /// 启动所有Mock服务器
    pub async fn start_all(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        for server in &self.servers {
            let handles = server.start_all().await?;
            self.handles.extend(handles);
        }
        Ok(())
    }

    /// 停止所有服务器
    pub async fn stop_all(&mut self) {
        for handle in &mut self.handles {
            handle.abort();
        }
        self.handles.clear();
    }

    /// 等待所有服务器退出
    pub async fn wait_all(&mut self) {
        for handle in &mut self.handles {
            let _ = handle.await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest;

    #[tokio::test]
    async fn test_mock_server_single_request() {
        let config = MockServerConfig {
            ports: vec![19933], // 使用不冲突的端口
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

        // 测试单个请求
        let client = reqwest::Client::new();
        let response = client
            .get("http://127.0.0.1:19933/callstack/0")
            .send()
            .await
            .unwrap();

        assert!(response.status().is_success());
        let flame_response: FlameGraphResponse = response.json().await.unwrap();
        assert_eq!(flame_response.rank, 0);
        assert!(!flame_response.stack.is_empty());
    }

    #[tokio::test]
    async fn test_mock_server_batch_request() {
        let config = MockServerConfig {
            ports: vec![19934],
            ranks_per_port: 100,
            max_stack_depth: 20,
            response_delay_ms: 0,
            error_rate: 0.0,
            stack_size_bytes: None,
        };

        let server = MockFlameGraphServer::new(config);
        let _handles = server.start_all().await.unwrap();

        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        let client = reqwest::Client::new();
        let response = client
            .get("http://127.0.0.1:19934/callstack?rank=0&batch_size=10")
            .send()
            .await
            .unwrap();

        assert!(response.status().is_success());
        let flame_responses: Vec<FlameGraphResponse> = response.json().await.unwrap();
        assert_eq!(flame_responses.len(), 10);
        
        for (i, resp) in flame_responses.iter().enumerate() {
            assert_eq!(resp.rank, i as u32);
            assert!(!resp.stack.is_empty());
        }
    }
}