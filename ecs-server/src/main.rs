mod handlers;
mod job_info_client;
mod state;

use axum::{
    Router,
    routing::{get, post},
};
use state::new_state;
use tower_http::cors::{Any, CorsLayer};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let shared = new_state();

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        // Push 接收端点
        .route("/push", post(handlers::push_handler))
        // JSON API
        .route("/api/collectors", get(handlers::api_collectors))
        .route("/api/collector/:id", get(handlers::api_collector))
        .route(
            "/api/collector/:id/flamegraph/all",
            get(handlers::api_flamegraph_all),
        )
        .route(
            "/api/collector/:id/flamegraph/:node_ip",
            get(handlers::api_flamegraph_node),
        )
        // HTML 页面
        .route("/", get(handlers::page_dashboard))
        .route("/collector/:id", get(handlers::page_collector))
        .layer(cors)
        .with_state(shared);

    let addr = std::env::var("ECS_ADDR").unwrap_or_else(|_| "0.0.0.0:4000".to_string());
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    tracing::info!("ECS 服务器启动: http://{}", addr);

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .await
    .unwrap();
}
