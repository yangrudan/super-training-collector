use crate::api::{get_node_ranks, get_rank_step_metrics, get_step_show_enabled};
use crate::components::common::*;
use crate::components::stack_view::StackAnalysisPanel;
use crate::models::*;
use leptos::prelude::*;
use leptos_router::hooks::use_params_map;

/// Level 3: Rank 详情视图
#[component]
pub fn Level3View() -> impl IntoView {
    let params = use_params_map();
    let (refresh_trigger, set_refresh_trigger) = signal(0u32);

    let ip = move || params.read().get("ip").unwrap_or_default();

    let ranks_resource = Resource::new(
        move || (ip(), refresh_trigger.get()),
        |(ip, _)| get_node_ranks(ip),
    );

    // Step 功能开关
    let step_enabled_resource = Resource::new(|| (), |_| get_step_show_enabled());

    let retry_callback = Callback::new(move |_| {
        set_refresh_trigger.update(|n| *n += 1);
    });

    view! {
        <div class="level3-view">
            <Breadcrumb items=vec![
                ("首页".to_string(), "/".to_string()),
                ("节点列表".to_string(), "/nodes".to_string()),
                (ip(), format!("/nodes/{}", ip())),
            ] />

            <Suspense fallback=move || view! { <Loading /> }>
                {move || {
                    let step_enabled = step_enabled_resource.get()
                        .and_then(|r| r.ok())
                        .unwrap_or(false);

                    ranks_resource.get().map(|result| {
                        match result {
                            Ok(response) => view! {
                                <div class="level3-content">
                                    // 节点概览
                                    <NodeOverview node=response.node.clone() />

                                    // 堆栈分析面板
                                    <StackAnalysisPanel node_ip=response.node.node_ip.clone() />

                                    // Rank 列表
                                    <section class="ranks-section">
                                        <h2>"Rank 详情"</h2>
                                        <div class="ranks-grid">
                                            {response.ranks.into_iter().map(|rank| {
                                                let node_ip = response.node.node_ip.clone();
                                                view! { <RankCardWithStep rank=rank node_ip=node_ip step_enabled=step_enabled /> }
                                            }).collect_view()}
                                        </div>
                                    </section>
                                </div>
                            }.into_any(),
                            Err(e) => view! {
                                <ErrorDisplay message=e.to_string() on_retry=retry_callback />
                            }.into_any(),
                        }
                    })
                }}
            </Suspense>
        </div>
    }
}

/// 节点概览组件
#[component]
fn NodeOverview(node: NodeMetrics) -> impl IntoView {
    view! {
        <section class="node-overview">
            <div class="node-header">
                <h1 class="node-title">
                    <StatusBadge status=node.status />
                    <span class="node-ip">{node.node_ip.clone()}</span>
                    <span class="node-hostname">"("{node.hostname.clone()}")"</span>
                </h1>
                <span class="rack-label">"主机: " {node.hostname.clone()}</span>
            </div>

            <div class="node-kpi-grid">
                <KpiCard
                    title="Rank 数量"
                    value=format!("{}", node.rank_count)
                />
                <KpiCard
                    title="平均 Step Time"
                    value=format!("{:.1}", node.avg_step_time_ms)
                    unit="ms"
                />
                <KpiCard
                    title="P99 Step Time"
                    value=format!("{:.1}", node.p99_step_time_ms)
                    unit="ms"
                />
                <KpiCard
                    title="平均 GPU 利用率"
                    value=format!("{:.1}", node.avg_gpu_utilization)
                    unit="%"
                />
                <KpiCard
                    title="慢 Rank 占比"
                    value=format!("{:.0}", node.slow_ratio * 100.0)
                    unit="%"
                />
                <KpiCard
                    title="平均 NCCL 延迟"
                    value=format!("{:.2}", node.avg_nccl_latency_ms)
                    unit="ms"
                />
            </div>
        </section>
    }
}

/// Rank 卡片组件
#[component]
fn RankCard(rank: RankMetrics) -> impl IntoView {
    let status_class = rank.status.css_class();

    view! {
        <div class=format!("rank-card {}", status_class)>
            <div class="rank-header">
                <span class="rank-id">"Rank " {rank.rank_id}</span>
                <span class="local-rank">"(GPU " {rank.local_rank} ")"</span>
                <StatusBadge status=rank.status />
            </div>

            <div class="rank-metrics">
                <div class="metric">
                    <span class="metric-label">"Step Time"</span>
                    <span class=step_time_class(rank.step_time_ms)>
                        {format!("{:.1} ms", rank.step_time_ms)}
                    </span>
                </div>
                <div class="metric">
                    <span class="metric-label">"相对 P50"</span>
                    <span class=ratio_class(rank.step_time_ratio)>
                        {format!("{:.2}x", rank.step_time_ratio)}
                    </span>
                </div>
                <div class="metric">
                    <span class="metric-label">"GPU 利用率"</span>
                    <span class=gpu_util_class(rank.gpu_utilization)>
                        {format!("{:.1}%", rank.gpu_utilization)}
                    </span>
                </div>
                <div class="metric">
                    <span class="metric-label">"显存"</span>
                    <span>
                        {format!("{:.1}/{:.0} GB", rank.gpu_memory_used_gb, rank.gpu_memory_total_gb)}
                    </span>
                </div>
                <div class="metric">
                    <span class="metric-label">"NCCL 延迟"</span>
                    <span class=nccl_class(rank.nccl_latency_ms)>
                        {format!("{:.2} ms", rank.nccl_latency_ms)}
                    </span>
                </div>
                <div class="metric">
                    <span class="metric-label">"当前 Step"</span>
                    <span>{rank.current_step}</span>
                </div>
            </div>

            {rank.error_message.map(|msg| view! {
                <div class="rank-error">
                    <span class="error-icon">"⚠"</span>
                    {msg}
                </div>
            })}
        </div>
    }
}

fn step_time_class(ms: f64) -> &'static str {
    if ms > 300.0 {
        "value-critical"
    } else if ms > 150.0 {
        "value-warning"
    } else {
        "value-healthy"
    }
}

fn ratio_class(ratio: f64) -> &'static str {
    if ratio > 3.0 {
        "value-critical"
    } else if ratio > 1.5 {
        "value-warning"
    } else {
        "value-healthy"
    }
}

fn gpu_util_class(util: f32) -> &'static str {
    if util < 50.0 {
        "value-critical"
    } else if util < 80.0 {
        "value-warning"
    } else {
        "value-healthy"
    }
}

fn nccl_class(ms: f64) -> &'static str {
    if ms > 10.0 {
        "value-critical"
    } else if ms > 5.0 {
        "value-warning"
    } else {
        "value-healthy"
    }
}

/// Rank 卡片组件（带 Step 信息）
#[component]
fn RankCardWithStep(rank: RankMetrics, node_ip: String, step_enabled: bool) -> impl IntoView {
    let status_class = rank.status.css_class();
    let rank_id = rank.rank_id;
    let local_rank = rank.local_rank;
    let ip_clone = node_ip.clone();

    // Step 指标资源（条件加载）
    let step_resource = Resource::new(
        move || (step_enabled, ip_clone.clone(), local_rank, rank_id),
        move |(enabled, ip, lr, rid)| async move {
            if enabled {
                get_rank_step_metrics(ip, lr, rid).await.ok()
            } else {
                None
            }
        },
    );

    view! {
        <div class=format!("rank-card {}", status_class)>
            <div class="rank-header">
                <span class="rank-id">"Rank " {rank.rank_id}</span>
                <span class="local-rank">"(GPU " {rank.local_rank} ")"</span>
                <StatusBadge status=rank.status />
            </div>

            <div class="rank-metrics">
                <div class="metric">
                    <span class="metric-label">"Step Time"</span>
                    <span class=step_time_class(rank.step_time_ms)>
                        {format!("{:.1} ms", rank.step_time_ms)}
                    </span>
                </div>
                <div class="metric">
                    <span class="metric-label">"相对 P50"</span>
                    <span class=ratio_class(rank.step_time_ratio)>
                        {format!("{:.2}x", rank.step_time_ratio)}
                    </span>
                </div>
                <div class="metric">
                    <span class="metric-label">"GPU 利用率"</span>
                    <span class=gpu_util_class(rank.gpu_utilization)>
                        {format!("{:.1}%", rank.gpu_utilization)}
                    </span>
                </div>
                <div class="metric">
                    <span class="metric-label">"显存"</span>
                    <span>
                        {format!("{:.1}/{:.0} GB", rank.gpu_memory_used_gb, rank.gpu_memory_total_gb)}
                    </span>
                </div>
                <div class="metric">
                    <span class="metric-label">"NCCL 延迟"</span>
                    <span class=nccl_class(rank.nccl_latency_ms)>
                        {format!("{:.2} ms", rank.nccl_latency_ms)}
                    </span>
                </div>
                <div class="metric">
                    <span class="metric-label">"当前 Step"</span>
                    <span>{rank.current_step}</span>
                </div>
            </div>

            // Step 详细信息（条件显示）
            <Suspense fallback=|| ()>
                {move || {
                    step_resource.get().flatten().map(|step_metrics| {
                        view! {
                            <div class="rank-step-details">
                                <h4>"Step 详情 (实时)"</h4>
                                <div class="step-detail-grid">
                                    <div class="step-detail">
                                        <span class="detail-label">"Step"</span>
                                        <span class="detail-value">{step_metrics.current_step}</span>
                                    </div>
                                    <div class="step-detail">
                                        <span class="detail-label">"Duration"</span>
                                        <span class="detail-value">
                                            {step_metrics.latest_duration_ms
                                                .map(|d| format!("{:.2} ms", d))
                                                .unwrap_or_else(|| "-".to_string())}
                                        </span>
                                    </div>
                                    <div class="step-detail">
                                        <span class="detail-label">"Allocated"</span>
                                        <span class="detail-value">
                                            {step_metrics.latest_allocated_gb
                                                .map(|a| format!("{:.2} GB", a))
                                                .unwrap_or_else(|| "-".to_string())}
                                        </span>
                                    </div>
                                </div>
                            </div>
                        }
                    })
                }}
            </Suspense>

            {rank.error_message.map(|msg| view! {
                <div class="rank-error">
                    <span class="error-icon">"⚠"</span>
                    {msg}
                </div>
            })}
        </div>
    }
}
