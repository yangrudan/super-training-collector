use leptos::prelude::*;
use leptos_router::hooks::use_navigate;
use crate::api::{get_nodes, get_all_nodes_flamegraph};
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
    let (refresh_trigger, set_refresh_trigger) = signal(0u32);

    let nodes_resource = Resource::new(
        move || (sort_field.get(), sort_order.get(), status_filter.get(), refresh_trigger.get()),
        |(field, order, filter, _)| get_nodes(Some(field), Some(order), Some(filter)),
    );

    let retry_callback = Callback::new(move |_| {
        set_refresh_trigger.update(|n| *n += 1);
    });

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
                                                <th>"主机名"</th>
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
                                <ErrorDisplay message=e.to_string() on_retry=retry_callback />
                            }.into_any(),
                        }
                    })
                }}
            </Suspense>
        </div>
    }
}

/// Tab 2: 全部节点 Rank 堆栈
/// IP 来自训练数据，端口由 config/collector.json 的 callstack_base_port 起始递增
#[component]
fn AllStacksTab() -> impl IntoView {
    // 全局合并火焰图状态
    let (combined_loading, set_combined_loading) = signal(false);
    let combined_svg: RwSignal<Option<String>> = RwSignal::new(None);
    let combined_error: RwSignal<Option<String>> = RwSignal::new(None);

    let on_generate_combined = move |_| {
        set_combined_loading.set(true);
        combined_svg.set(None);
        combined_error.set(None);
        leptos::task::spawn_local(async move {
            match get_all_nodes_flamegraph().await {
                Ok(svg) => combined_svg.set(Some(svg)),
                Err(e) => combined_error.set(Some(e.to_string())),
            }
            set_combined_loading.set(false);
        });
    };

    view! {
        <div class="all-stacks-tab">
            <div class="all-stacks-toolbar">
                <button
                    class="collect-btn collect-btn-combined"
                    on:click=on_generate_combined
                    disabled=move || combined_loading.get()
                >
                    {move || if combined_loading.get() { "合并生成中..." } else { "生成全局合并火焰图" }}
                </button>
            </div>

            // 全局合并火焰图展示区
            <Show when=move || combined_loading.get()>
                <Loading />
            </Show>
            <Show when=move || combined_error.get().is_some()>
                <div class="stack-error">
                    {move || combined_error.get().unwrap_or_default()}
                </div>
            </Show>
            <Show when=move || combined_svg.get().is_some()>
                <div class="flamegraph-result-box combined-flamegraph-box">
                    <div class="flamegraph-result-title">"全局合并火焰图 (所有节点所有 Rank)"</div>
                    <div
                        class="flamegraph-svg"
                        inner_html=move || combined_svg.get().unwrap_or_default()
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
            <td>{node.hostname.clone()}</td>
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
