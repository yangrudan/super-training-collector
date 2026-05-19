use app::*;
use axum::extract::Path;
use axum::response::{IntoResponse, Response};
use axum::Router;
use axum::http::{header, StatusCode};
use leptos::logging::log;
use leptos::prelude::*;
use leptos_axum::{generate_route_list, LeptosRoutes};

/// REST 处理器：获取指定节点的火焰图 SVG
async fn rest_flamegraph_node(Path(ip): Path<String>) -> Response {
    use app::flamegraph::{
        build_callstack_urls, collect_and_generate_flamegraph, get_config_path,
        load_collector_config,
    };
    use app::adapter::get_real_training_data;

    let config = match load_collector_config(&get_config_path()) {
        Ok(c) => c,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to load config: {}", e),
            )
                .into_response();
        }
    };

    let rank_count = match get_real_training_data().await {
        Ok((_, nodes)) => nodes
            .into_iter()
            .find(|n| n.node_ip == ip)
            .map(|n| n.rank_count)
            .unwrap_or(4),
        Err(e) => {
            return (
                StatusCode::BAD_GATEWAY,
                format!("Failed to get training data: {}", e),
            )
                .into_response();
        }
    };

    let urls = build_callstack_urls(&ip, rank_count, config.callstack_base_port);

    match collect_and_generate_flamegraph(&ip, urls, Some(config.batch_size)).await {
        Ok(svg) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "image/svg+xml")],
            svg,
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to generate flamegraph: {}", e),
        )
            .into_response(),
    }
}

/// REST 处理器：获取所有节点合并火焰图 SVG
async fn rest_flamegraph_all() -> Response {
    use app::flamegraph::{
        collect_and_generate_flamegraph, get_config_path, load_collector_config,
    };
    use app::adapter::get_real_training_data;

    let config = match load_collector_config(&get_config_path()) {
        Ok(c) => c,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to load config: {}", e),
            )
                .into_response();
        }
    };

    let ranks = match get_real_training_data().await {
        Ok((ranks, _)) => ranks,
        Err(e) => {
            return (
                StatusCode::BAD_GATEWAY,
                format!("Failed to get training data: {}", e),
            )
                .into_response();
        }
    };

    if ranks.is_empty() {
        return (StatusCode::NOT_FOUND, "No ranks found").into_response();
    }

    let urls: Vec<String> = ranks
        .iter()
        .map(|r| {
            format!(
                "http://{}:{}/apis/pythonext/callstack",
                r.node_ip,
                config.callstack_base_port + r.local_rank as u16
            )
        })
        .collect();

    match collect_and_generate_flamegraph("all_nodes", urls, Some(config.batch_size)).await {
        Ok(svg) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "image/svg+xml")],
            svg,
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to generate flamegraph: {}", e),
        )
            .into_response(),
    }
}

#[tokio::main]
async fn main() {
    // 处理 --version 参数
    if std::env::args().any(|arg| arg == "--version" || arg == "-V") {
        println!("super-training-collector {}", env!("CARGO_PKG_VERSION"));
        return;
    }

    // 初始化日志，从 RUST_LOG 环境变量读取级别，默认为 warn
    // 例如：RUST_LOG=warn 或 RUST_LOG=super_trainning_collector=debug
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .init();

    // 启动 HANG 检测调度器
    tokio::spawn(async { app::hang_detector::start_hang_detector_scheduler().await });

    // 启动数据推送调度器（目标 URL 为空时自动禁用）
    tokio::spawn(async { app::push_scheduler::start_push_scheduler().await });

    let conf = get_configuration(None).unwrap();
    let addr = conf.leptos_options.site_addr;
    let leptos_options = conf.leptos_options;
    // Generate the list of routes in your Leptos App
    let routes = generate_route_list(App);

    let app = Router::new()
        .route("/rest/flamegraph/all", axum::routing::get(rest_flamegraph_all))
        .route("/rest/flamegraph/{ip}", axum::routing::get(rest_flamegraph_node))
        .leptos_routes(&leptos_options, routes, {
            let leptos_options = leptos_options.clone();
            move || shell(leptos_options.clone())
        })
        .fallback(leptos_axum::file_and_error_handler(shell))
        .with_state(leptos_options);

    // run our app with hyper
    // `axum::Server` is a re-export of `hyper::Server`
    log!("listening on http://{}", &addr);
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(listener, app.into_make_service())
        .await
        .unwrap();
}
