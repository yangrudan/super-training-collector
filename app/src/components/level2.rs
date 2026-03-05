use leptos::prelude::*;
use leptos_router::hooks::use_navigate;
use crate::api::{get_nodes, get_config_nodes, get_node_flamegraph};
use crate::models::*;
use crate::components::common::*;

/// Level 2: 节点聚合视图 (带 Tab)
#[component]
pub fn Level2View() -> impl IntoView {
    let active_tab = RwSignal::new(0u8); // 0 = 节点列表, 1 = 全部堆栈

    view! {
        <div class="level2-view">
            <Breadcrumb items=vec![
                ("首页".to_string(), "/".to_string()),
                ("节点列表".to_string(), "/nodes".to_string()),
            ] />

            <h1 class="page-title">"节点状态监控"</h1>

            // Tab 导航
            <div class="tab-bar">
                <button
                    class=move || if active_tab.get() == 0 { "tab-btn tab-btn-active" } else { "tab-btn" }
                    on:click=move |_| active_tab.set(0)
                >"节点列表"</button>
                <button
                    class=move || if active_tab.get() == 1 { "tab-btn tab-btn-active" } else { "tab-btn" }
                    on:click=move |_| active_tab.set(1)
                >"全部 Rank 堆栈"</button>
            </div>

            // Tab 内容
            <Show when=move || active_tab.get() == 0>
                <NodesTableTab />
            </Show>
            <Show when=move || active_tab.get() == 1>
                <AllStacksTab />
            </Show>
        </div>
    }
}

/// Tab 1: 节点表格
#[component]
fn NodesTableTab() -> impl IntoView {
    let sort_field = RwSignal::new(SortField::SlowRatio);
    let sort_order = RwSignal::new(SortOrder::Desc);
    let status_filter = RwSignal::new(StatusFilter::All);

    let nodes_resource = Resource::new(
        move || (sort_field.get(), sort_order.get(), status_filter.get()),
        |(field, order, filter)| get_nodes(Some(field), Some(order), Some(filter)),
    );

    view! {
        <div>
            // 筛选栏
            <div class="filter-bar">
                <div class="filter-group">
                    <label>"状态筛选:"</label>
                    <select
                        on:change=move |ev| {
                            let value = event_target_value(&ev);
                            status_filter.set(match value.as_str() {
                                "healthy" => StatusFilter::Healthy,
                                "warning" => StatusFilter::Warning,
                                "critical" => StatusFilter::Critical,
                                _ => StatusFilter::All,
                            });
                        }
                    >
                        <option value="all">"全部"</option>
                        <option value="healthy">"正常"</option>
                        <option value="warning">"警告"</option>
                        <option value="critical">"故障"</option>
                    </select>
                </div>

                <div class="filter-group">
                    <label>"排序方式:"</label>
                    <select
                        on:change=move |ev| {
                            let value = event_target_value(&ev);
                            sort_field.set(match value.as_str() {
                                "step_time" => SortField::StepTime,
                                "gpu_util" => SortField::GpuUtilization,
                                "nccl" => SortField::NcclLatency,
                                _ => SortField::SlowRatio,
                            });
                        }
                    >
                        <option value="slow_ratio">"慢占比"</option>
                        <option value="step_time">"P99 Step Time"</option>
                        <option value="gpu_util">"GPU 利用率"</option>
                        <option value="nccl">"NCCL 延迟"</option>
                    </select>
                </div>

                <div class="filter-group">
                    <label>"排序方向:"</label>
                    <select
                        on:change=move |ev| {
                            let value = event_target_value(&ev);
                            sort_order.set(if value == "asc" {
                                SortOrder::Asc
                            } else {
                                SortOrder::Desc
                            });
                        }
                    >
                        <option value="desc">"降序"</option>
                        <option value="asc">"升序"</option>
                    </select>
                </div>
            </div>

            // 节点表格
            <Suspense fallback=move || view! { <Loading /> }>
                {move || {
                    nodes_resource.get().map(|result| {
                        match result {
                            Ok(response) => view! {
                                <div class="nodes-table-container">
                                    <table class="nodes-table">
                                        <thead>
                                            <tr>
                                                <th>"状态"</th>
                                                <th>"节点 IP"</th>
                                                <th>"机柜"</th>
                                                <th>"慢占比"</th>
                                                <th>"P50 (ms)"</th>
                                                <th>"P99 (ms)"</th>
                                                <th>"GPU 利用率"</th>
                                                <th>"NCCL 延迟"</th>
                                                <th>"Rank 状态"</th>
                                            </tr>
                                        </thead>
                                        <tbody>
                                            {response.nodes.into_iter().map(|node| {
                                                view! { <NodeRow node=node /> }
                                            }).collect_view()}
                                        </tbody>
                                    </table>
                                    <div class="table-footer">
                                        "共 " {response.total} " 个节点"
                                    </div>
                                </div>
                            }.into_any(),
                            Err(e) => view! {
                                <ErrorDisplay message=e.to_string() />
                            }.into_any(),
                        }
                    })
                }}
            </Suspense>
        </div>
    }
}

/// Tab 2: 全部节点 Rank 堆栈 (从 config/flamegraph.json 加载)
#[component]
fn AllStacksTab() -> impl IntoView {
    let config_resource = Resource::new(|| (), |_| get_config_nodes());

    view! {
        <div class="all-stacks-tab">
            <Suspense fallback=move || view! { <Loading /> }>
                {move || {
                    config_resource.get().map(|result| {
                        match result {
                            Ok(nodes) => view! {
                                <div class="all-stacks-grid">
                                    {nodes.into_iter().map(|(ip, urls)| {
                                        view! { <NodeFlamegraphPanel ip=ip urls=urls /> }
                                    }).collect_view()}
                                </div>
                            }.into_any(),
                            Err(e) => view! {
                                <ErrorDisplay message=e.to_string() />
                            }.into_any(),
                        }
                    })
                }}
            </Suspense>
        </div>
    }
}

/// 单节点火焰图面板 (懒加载)
#[component]
fn NodeFlamegraphPanel(ip: String, urls: Vec<String>) -> impl IntoView {
    let (loading, set_loading) = signal(false);
    let flamegraph_svg: RwSignal<Option<String>> = RwSignal::new(None);
    let error_msg: RwSignal<Option<String>> = RwSignal::new(None);

    let ip_clone = ip.clone();
    let on_generate = move |_| {
        let ip = ip_clone.clone();
        set_loading.set(true);
        flamegraph_svg.set(None);
        error_msg.set(None);
        leptos::task::spawn_local(async move {
            match get_node_flamegraph(ip).await {
                Ok(svg) => {
                    flamegraph_svg.set(Some(svg));
                }
                Err(e) => {
                    error_msg.set(Some(e.to_string()));
                }
            }
            set_loading.set(false);
        });
    };

    let rank_count = urls.len();

    view! {
        <div class="node-flamegraph-panel">
            <div class="node-fg-header">
                <div class="node-fg-info">
                    <span class="node-ip">{ip.clone()}</span>
                    <span class="rank-count-badge">
                        {rank_count} " 个 Rank"
                    </span>
                </div>
                <button
                    class="collect-btn"
                    on:click=on_generate
                    disabled=move || loading.get()
                >
                    {move || if loading.get() { "生成中..." } else { "生成火焰图" }}
                </button>
            </div>

            // Rank URL 列表
            <div class="rank-urls">
                {urls.into_iter().enumerate().map(|(i, url)| {
                    view! {
                        <div class="rank-url-item">
                            <span class="rank-label">"Rank " {i}</span>
                            <span class="rank-url-text">{url}</span>
                        </div>
                    }
                }).collect_view()}
            </div>

            // 错误信息
            <Show when=move || error_msg.get().is_some()>
                <div class="stack-error">
                    {move || error_msg.get().unwrap_or_default()}
                </div>
            </Show>

            // 火焰图 SVG
            <Show when=move || flamegraph_svg.get().is_some()>
                <div class="flamegraph-container">
                    <div
                        class="flamegraph-svg"
                        inner_html=move || flamegraph_svg.get().unwrap_or_default()
                    />
                </div>
            </Show>
        </div>
    }
}

/// 节点行组件
#[component]
fn NodeRow(node: NodeMetrics) -> impl IntoView {
    let navigate = use_navigate();
    let ip = node.node_ip.clone();

    let on_click = move |_| {
        let ip = ip.clone();
        navigate(&format!("/nodes/{}", ip), Default::default());
    };

    view! {
        <tr class="node-row clickable" on:click=on_click>
            <td>
                <StatusBadge status=node.status />
            </td>
            <td class="node-ip">{node.node_ip.clone()}</td>
            <td>{node.rack_id.clone()}</td>
            <td>
                <HeatCell value=node.slow_ratio show_value=true />
            </td>
            <td>{format!("{:.1}", node.p50_step_time_ms)}</td>
            <td class=step_time_class(node.p99_step_time_ms)>
                {format!("{:.1}", node.p99_step_time_ms)}
            </td>
            <td class=gpu_util_class(node.avg_gpu_utilization)>
                {format!("{:.1}%", node.avg_gpu_utilization)}
            </td>
            <td>{format!("{:.2} ms", node.avg_nccl_latency_ms)}</td>
            <td>
                <span class="rank-status">
                    <span class="healthy">{node.healthy_count}</span>
                    "/"
                    <span class="warning">{node.warning_count}</span>
                    "/"
                    <span class="critical">{node.critical_count}</span>
                </span>
            </td>
        </tr>
    }
}

fn step_time_class(ms: f64) -> &'static str {
    if ms > 300.0 {
        "value-critical"
    } else if ms > 150.0 {
        "value-warning"
    } else {
        ""
    }
}

fn gpu_util_class(util: f32) -> &'static str {
    if util < 50.0 {
        "value-critical"
    } else if util < 80.0 {
        "value-warning"
    } else {
        ""
    }
}
