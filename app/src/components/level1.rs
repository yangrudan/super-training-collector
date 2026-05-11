use crate::api::{get_global_metrics, get_global_step_metrics, get_step_show_enabled};
use crate::components::common::*;
use crate::components::hang_indicator::HangIndicatorCompact;
use crate::components::rank_analysis::RankAnalysisSummary;
use leptos::prelude::*;

/// Level 1: 全局态势视图
#[component]
pub fn Level1View() -> impl IntoView {
    let (refresh_trigger, set_refresh_trigger) = signal(0u32);

    let global_resource = Resource::new(move || refresh_trigger.get(), |_| get_global_metrics());
    let step_enabled_resource = Resource::new(|| (), |_| get_step_show_enabled());
    let step_resource = Resource::new(move || refresh_trigger.get(), |_| get_global_step_metrics());

    let retry_callback = Callback::new(move |_| {
        set_refresh_trigger.update(|n| *n += 1);
    });

    view! {
        <div class="dashboard-page">
            <section class="page-hero panel-surface">
                <div class="page-hero-copy">
                    <div class="page-eyebrow">"全局总览"</div>
                    <h1 class="page-title">"训练任务监控总台"</h1>
                    <p class="page-description">
                        "聚焦节点健康、GPU 利用率与训练进度，帮助运维和研发快速判断是否需要下钻排障。"
                    </p>
                </div>

                <div class="page-hero-side">
                    <div class="hero-inline-status">
                        <span class="hero-status-label">"HANG 检测"</span>
                        <HangIndicatorCompact />
                    </div>
                    <a href="/nodes" class="primary-action">"进入节点控制台"</a>
                </div>
            </section>

            <Suspense fallback=move || view! { <Loading /> }>
                {move || {
                    global_resource.get().map(|result| {
                        match result {
                            Ok(metrics) => {
                                let node_status = if metrics.critical_nodes > 0 {
                                    crate::models::HealthStatus::Critical
                                } else if metrics.warning_nodes > 0 {
                                    crate::models::HealthStatus::Warning
                                } else {
                                    crate::models::HealthStatus::Healthy
                                };

                                view! {
                                <div class="page-grid">
                                    <section class="summary-strip">
                                        <KpiCard
                                            title="在线节点"
                                            value=metrics.total_nodes.to_string()
                                            status=node_status
                                        />
                                        <KpiCard
                                            title="活跃 Rank"
                                            value=metrics.total_ranks.to_string()
                                        />
                                        <KpiCard
                                            title="全局 P99 Step"
                                            value=format!("{:.1}", metrics.global_p99_step_time_ms)
                                            unit="ms"
                                        />
                                        <KpiCard
                                            title="平均 GPU 利用率"
                                            value=format!("{:.1}", metrics.global_avg_gpu_utilization)
                                            unit="%"
                                        />
                                    </section>

                                    <section class="panel-surface">
                                        <div class="panel-header-line">
                                            <div>
                                                <div class="section-label">"健康态势"</div>
                                                <h2 class="section-title">"节点与 Rank 状态分布"</h2>
                                            </div>
                                            <div class="panel-stat">
                                                "慢节点占比 " {format!("{:.0}%", metrics.slow_node_ratio * 100.0)}
                                            </div>
                                        </div>

                                        <div class="health-grid">
                                            <HealthDistribution
                                                healthy=metrics.healthy_nodes
                                                warning=metrics.warning_nodes
                                                critical=metrics.critical_nodes
                                                label="节点"
                                            />
                                            <HealthDistribution
                                                healthy=metrics.healthy_ranks
                                                warning=metrics.warning_ranks
                                                critical=metrics.critical_ranks
                                                label="Rank"
                                            />
                                        </div>
                                    </section>

                                    <section class="panel-surface">
                                        <div class="panel-header-line">
                                            <div>
                                                <div class="section-label">"性能指标"</div>
                                                <h2 class="section-title">"核心性能采样"</h2>
                                            </div>
                                        </div>
                                        <div class="kpi-grid">
                                            <KpiCard
                                                title="P50 Step Time"
                                                value=format!("{:.1}", metrics.global_p50_step_time_ms)
                                                unit="ms"
                                            />
                                            <KpiCard
                                                title="P99 Step Time"
                                                value=format!("{:.1}", metrics.global_p99_step_time_ms)
                                                unit="ms"
                                            />
                                            <KpiCard
                                                title="平均 GPU 利用率"
                                                value=format!("{:.1}", metrics.global_avg_gpu_utilization)
                                                unit="%"
                                            />
                                            <KpiCard
                                                title="慢节点占比"
                                                value=format!("{:.0}", metrics.slow_node_ratio * 100.0)
                                                unit="%"
                                            />
                                        </div>
                                    </section>

                                    <section class="panel-surface">
                                        <div class="panel-header-line">
                                            <div>
                                                <div class="section-label">"训练进度"</div>
                                                <h2 class="section-title">"基础训练状态"</h2>
                                            </div>
                                        </div>
                                        <div class="progress-grid">
                                            <Suspense fallback=move || view! {
                                                <KpiCard title="当前 Step" value=metrics.current_step.to_string() />
                                            }>
                                                {move || {
                                                    let enabled = step_enabled_resource
                                                        .get()
                                                        .and_then(|r| r.ok())
                                                        .unwrap_or(false);

                                                    if enabled {
                                                        step_resource.get().map(|result| {
                                                            match result {
                                                                Ok(step_metrics) if step_metrics.current_step > 0 => view! {
                                                                    <KpiCard
                                                                        title="当前 Step"
                                                                        value=step_metrics.current_step.to_string()
                                                                    />
                                                                }
                                                                    .into_any(),
                                                                _ => view! {
                                                                    <KpiCard
                                                                        title="当前 Step"
                                                                        value=metrics.current_step.to_string()
                                                                    />
                                                                }
                                                                    .into_any(),
                                                            }
                                                        })
                                                    } else {
                                                        Some(view! {
                                                            <KpiCard
                                                                title="当前 Step"
                                                                value=metrics.current_step.to_string()
                                                            />
                                                        }
                                                            .into_any())
                                                    }
                                                }}
                                            </Suspense>
                                            <KpiCard
                                                title="训练速度"
                                                value=format!("{:.2}", metrics.steps_per_second)
                                                unit="steps/s"
                                            />
                                            <KpiCard
                                                title="预计剩余时间"
                                                value=metrics.estimated_remaining_hours
                                                    .map(|h| format!("{:.1}", h))
                                                    .unwrap_or_else(|| "-".to_string())
                                                unit="小时"
                                            />
                                        </div>
                                    </section>

                                    <StepMetricsSection
                                        step_enabled_resource=step_enabled_resource
                                        step_resource=step_resource
                                    />
                                    // 问题 Rank 分析摘要（有结果时自动显示）
                                    <RankAnalysisSummary />

                                    // 快速导航
                                    <section class="nav-section">
                                        <a href="/nodes" class="nav-button">
                                            "查看节点详情 →"
                                        </a>
                                    </section>
                                </div>
                            }
                                .into_any()
                            }
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

/// Step 指标区域组件（条件显示）
#[component]
fn StepMetricsSection(
    step_enabled_resource: Resource<Result<bool, leptos::prelude::ServerFnError>>,
    step_resource: Resource<
        Result<crate::models::GlobalStepMetrics, leptos::prelude::ServerFnError>,
    >,
) -> impl IntoView {
    view! {
        <Suspense fallback=|| ()>
            {move || {
                let enabled = step_enabled_resource
                    .get()
                    .and_then(|r| r.ok())
                    .unwrap_or(false);

                if !enabled {
                    return None;
                }

                Some(view! {
                    <section class="panel-surface step-metrics-section">
                        <div class="panel-header-line">
                            <div>
                                <div class="section-label">"实时 Step"</div>
                                <h2 class="section-title">"Step 细粒度指标"</h2>
                            </div>
                        </div>

                        <Suspense fallback=move || view! { <div class="loading-small">"加载中..."</div> }>
                            {move || {
                                step_resource.get().map(|result| {
                                    match result {
                                        Ok(step_metrics) => view! {
                                            <div class="step-metrics-grid">
                                                <KpiCard
                                                    title="当前 Step"
                                                    value=step_metrics.current_step.to_string()
                                                />
                                                <KpiCard
                                                    title="最近 Duration"
                                                    value=step_metrics.latest_duration_ms
                                                        .map(|d| format!("{:.2}", d))
                                                        .unwrap_or_else(|| "-".to_string())
                                                    unit="ms"
                                                />
                                                <KpiCard
                                                    title="显存分配"
                                                    value=step_metrics.latest_allocated_gb
                                                        .map(|a| format!("{:.2}", a))
                                                        .unwrap_or_else(|| "-".to_string())
                                                    unit="GB"
                                                />
                                            </div>

                                            {if !step_metrics.records.is_empty() {
                                                Some(view! {
                                                    <div class="step-records-table">
                                                        <div class="panel-header-line compact">
                                                            <div>
                                                                <div class="section-label">"明细表"</div>
                                                                <h3 class="section-title small">"最近 Step 记录"</h3>
                                                            </div>
                                                        </div>
                                                        <div class="table-shell">
                                                            <table class="nodes-table">
                                                                <thead>
                                                                    <tr>
                                                                        <th>"Step"</th>
                                                                        <th>"Module"</th>
                                                                        <th>"Stage"</th>
                                                                        <th>"Duration (ms)"</th>
                                                                        <th>"Allocated (GB)"</th>
                                                                    </tr>
                                                                </thead>
                                                                <tbody>
                                                                    {step_metrics.records.iter().map(|record| {
                                                                        view! {
                                                                            <tr>
                                                                                <td class="mono-cell">{record.step}</td>
                                                                                <td>{record.module.clone().unwrap_or_else(|| "-".to_string())}</td>
                                                                                <td>{record.stage.clone().unwrap_or_else(|| "-".to_string())}</td>
                                                                                <td>
                                                                                    {record.duration
                                                                                        .map(|d| format!("{:.2}", d / 1000.0))
                                                                                        .unwrap_or_else(|| "-".to_string())}
                                                                                </td>
                                                                                <td>
                                                                                    {record.allocated
                                                                                        .map(|a| format!("{:.2}", a as f64 / 1024.0 / 1024.0 / 1024.0))
                                                                                        .unwrap_or_else(|| "-".to_string())}
                                                                                </td>
                                                                            </tr>
                                                                        }
                                                                    }).collect_view()}
                                                                </tbody>
                                                            </table>
                                                        </div>
                                                    </div>
                                                })
                                            } else {
                                                None
                                            }}
                                        }
                                            .into_any(),
                                        Err(_) => view! {
                                            <div class="step-metrics-error">"无法获取 Step 指标数据"</div>
                                        }
                                            .into_any(),
                                    }
                                })
                            }}
                        </Suspense>
                    </section>
                })
            }}
        </Suspense>
    }
}
