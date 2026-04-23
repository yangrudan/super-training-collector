use crate::api::{get_all_nodes_flamegraph, get_nodes};
use crate::components::common::*;
use crate::models::*;
use leptos::prelude::*;
use leptos_router::hooks::use_navigate;

/// Level 2: 节点聚合视图 (带 Tab)
#[component]
pub fn Level2View() -> impl IntoView {
    let active_tab = RwSignal::new(0u8);

    view! {
        <div class="nodes-page">
            <Breadcrumb items=vec![
                ("首页".to_string(), "/".to_string()),
                ("节点控制台".to_string(), "/nodes".to_string()),
            ] />

            <section class="page-hero panel-surface compact">
                <div class="page-hero-copy">
                    <div class="page-eyebrow">"节点巡检"</div>
                    <h1 class="page-title">"节点状态监控"</h1>
                    <p class="page-description">
                        "按节点聚合展示训练状态、性能热点和堆栈诊断入口，适合快速筛选异常机器。"
                    </p>
                </div>
            </section>

            <div class="tab-bar">
                <button
                    class=move || {
                        if active_tab.get() == 0 {
                            "tab-btn tab-btn-active"
                        } else {
                            "tab-btn"
                        }
                    }
                    on:click=move |_| active_tab.set(0)
                >
                    "节点列表"
                </button>
                <button
                    class=move || {
                        if active_tab.get() == 1 {
                            "tab-btn tab-btn-active"
                        } else {
                            "tab-btn"
                        }
                    }
                    on:click=move |_| active_tab.set(1)
                >
                    "全部 Rank 堆栈"
                </button>
            </div>

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
        move || {
            (
                sort_field.get(),
                sort_order.get(),
                status_filter.get(),
                refresh_trigger.get(),
            )
        },
        |(field, order, filter, _)| get_nodes(Some(field), Some(order), Some(filter)),
    );

    let retry_callback = Callback::new(move |_| {
        set_refresh_trigger.update(|n| *n += 1);
    });

    view! {
        <div class="panel-stack">
            <div class="filter-bar panel-surface">
                <div class="filter-group">
                    <label>"状态"</label>
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
                        <option value="warning">"挂起"</option>
                        <option value="critical">"异常"</option>
                    </select>
                </div>

                <div class="filter-group">
                    <label>"排序指标"</label>
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
                    <label>"排序方向"</label>
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

            <Suspense fallback=move || view! { <Loading /> }>
                {move || {
                    nodes_resource.get().map(|result| {
                        match result {
                            Ok(response) => view! {
                                <section class="table-shell panel-surface">
                                    <div class="panel-header-line">
                                        <div>
                                            <div class="section-label">"节点列表"</div>
                                            <h2 class="section-title">"节点状态与热点指标"</h2>
                                        </div>
                                        <div class="panel-stat">"共 " {response.total} " 个节点"</div>
                                    </div>

                                    <div class="table-scroll">
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
                                    </div>
                                </section>
                            }
                                .into_any(),
                            Err(e) => view! {
                                <ErrorDisplay message=e.to_string() on_retry=retry_callback />
                            }
                                .into_any(),
                        }
                    })
                }}
            </Suspense>
        </div>
    }
}

/// Tab 2: 全部节点 Rank 堆栈
#[component]
fn AllStacksTab() -> impl IntoView {
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
        <section class="panel-surface all-stacks-tab">
            <div class="panel-header-line">
                <div>
                    <div class="section-label">"堆栈诊断"</div>
                    <h2 class="section-title">"全局合并火焰图"</h2>
                </div>
                <div class="all-stacks-toolbar">
                    <button
                        class="collect-btn collect-btn-combined"
                        on:click=on_generate_combined
                        disabled=move || combined_loading.get()
                    >
                        {move || {
                            if combined_loading.get() {
                                "生成中..."
                            } else {
                                "生成全局合并火焰图"
                            }
                        }}
                    </button>

                    <Show when=move || combined_svg.get().is_some()>
                        <button
                            class="collect-btn subtle"
                            on:click=move |_| {
                                if let Some(svg_content) = combined_svg.get() {
                                    #[cfg(feature = "hydrate")]
                                    {
                                        use wasm_bindgen::JsCast;

                                        let filename = format!("flamegraph_all_nodes_{}.svg", {
                                            use js_sys::Date;
                                            let date = Date::new_0();
                                            format!(
                                                "{:04}{:02}{:02}{:02}{:02}{:02}",
                                                date.get_full_year(),
                                                date.get_month() + 1,
                                                date.get_date(),
                                                date.get_hours(),
                                                date.get_minutes(),
                                                date.get_seconds()
                                            )
                                        });
                                        let document = web_sys::window().unwrap().document().unwrap();
                                        let mut blob_options = web_sys::BlobPropertyBag::new();
                                        blob_options.set_type("image/svg+xml");
                                        let blob = web_sys::Blob::new_with_str_sequence_and_options(
                                            &js_sys::Array::of1(&svg_content.into()),
                                            &blob_options,
                                        )
                                            .unwrap();
                                        let url =
                                            web_sys::Url::create_object_url_with_blob(&blob).unwrap();

                                        let a = document
                                            .create_element("a")
                                            .unwrap()
                                            .dyn_into::<web_sys::HtmlAnchorElement>()
                                            .unwrap();
                                        a.set_href(&url);
                                        a.set_download(&filename);
                                        a.click();

                                        web_sys::Url::revoke_object_url(&url).unwrap();
                                    }
                                }
                            }
                            title="下载全局合并火焰图 SVG"
                        >
                            "下载 SVG"
                        </button>
                    </Show>
                </div>
            </div>

            <p class="section-note">
                "使用统一的火焰图视图汇总全部节点全部 Rank 的调用栈，便于识别集群级瓶颈。"
            </p>

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
        </section>
    }
}

/// 节点行组件
#[component]
fn NodeRow(node: NodeMetrics) -> impl IntoView {
    let navigate = use_navigate();
    let ip = node.node_ip.clone();

    let on_click = move |_| {
        navigate(&format!("/nodes/{}", ip), Default::default());
    };

    view! {
        <tr class="node-row clickable" on:click=on_click>
            <td>
                <StatusBadge status=node.status />
            </td>
            <td class="node-ip-cell">
                <span class="mono-cell">{node.node_ip.clone()}</span>
                <CopyButton value=node.node_ip.clone() label="复制IP" />
            </td>
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
        "value-healthy"
    }
}
