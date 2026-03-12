use leptos::prelude::*;
use crate::api::{get_global_metrics, get_step_show_enabled, get_global_step_metrics};
use crate::components::common::*;

/// Level 1: 全局态势视图
#[component]
pub fn Level1View() -> impl IntoView {
    let (refresh_trigger, set_refresh_trigger) = signal(0u32);
    
    let global_resource = Resource::new(
        move || refresh_trigger.get(),
        |_| get_global_metrics()
    );

    // Step 功能开关资源
    let step_enabled_resource = Resource::new(|| (), |_| get_step_show_enabled());
    
    // Step 指标资源（仅当 step_enabled 为 true 时有效）
    let step_resource = Resource::new(
        move || refresh_trigger.get(),
        |_| get_global_step_metrics()
    );

    let retry_callback = Callback::new(move |_| {
        set_refresh_trigger.update(|n| *n += 1);
    });

    view! {
        <div class="level1-view">
            <h1 class="page-title">"训练任务监控面板"</h1>

            <Suspense fallback=move || view! { <Loading /> }>
                {move || {
                    global_resource.get().map(|result| {
                        match result {
                            Ok(metrics) => view! {
                                <div class="level1-content">
                                    // 健康分布区域
                                    <section class="health-section">
                                        <h2>"健康状态分布"</h2>
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

                                    // KPI 指标区域
                                    <section class="kpi-section">
                                        <h2>"全局性能指标"</h2>
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

                                    // 训练进度区域（基础信息）
                                    <section class="progress-section">
                                        <h2>"训练进度"</h2>
                                        <div class="progress-grid">
                                            <KpiCard
                                                title="当前 Step"
                                                value=format!("{}", metrics.current_step)
                                            />
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

                                    // Step 详细指标（Phase 2，条件显示）
                                    <StepMetricsSection 
                                        step_enabled_resource=step_enabled_resource
                                        step_resource=step_resource
                                    />

                                    // 快速导航
                                    <section class="nav-section">
                                        <a href="/nodes" class="nav-button">
                                            "查看节点详情 →"
                                        </a>
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

/// Step 指标区域组件（条件显示）
#[component]
fn StepMetricsSection(
    step_enabled_resource: Resource<Result<bool, leptos::prelude::ServerFnError>>,
    step_resource: Resource<Result<crate::models::GlobalStepMetrics, leptos::prelude::ServerFnError>>,
) -> impl IntoView {
    view! {
        <Suspense fallback=|| ()>
            {move || {
                let enabled = step_enabled_resource.get()
                    .and_then(|r| r.ok())
                    .unwrap_or(false);
                
                if !enabled {
                    return None;
                }
                
                Some(view! {
                    <section class="step-metrics-section">
                        <h2>"Step 详细指标"</h2>
                        <Suspense fallback=move || view! { <div class="loading-small">"加载中..."</div> }>
                            {move || {
                                step_resource.get().map(|result| {
                                    match result {
                                        Ok(step_metrics) => view! {
                                            <div class="step-metrics-grid">
                                                <KpiCard
                                                    title="当前 Step (实时)"
                                                    value=format!("{}", step_metrics.current_step)
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
                                            
                                            // Step 历史记录表格
                                            {if !step_metrics.records.is_empty() {
                                                Some(view! {
                                                    <div class="step-records-table">
                                                        <h3>"最近 Step 记录"</h3>
                                                        <table>
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
                                                                            <td>{record.step}</td>
                                                                            <td>{record.module.clone().unwrap_or_else(|| "-".to_string())}</td>
                                                                            <td>{record.stage.clone().unwrap_or_else(|| "-".to_string())}</td>
                                                                            <td>{record.duration.map(|d| format!("{:.2}", d / 1000.0)).unwrap_or_else(|| "-".to_string())}</td>
                                                                            <td>{record.allocated.map(|a| format!("{:.2}", a as f64 / 1024.0 / 1024.0 / 1024.0)).unwrap_or_else(|| "-".to_string())}</td>
                                                                        </tr>
                                                                    }
                                                                }).collect_view()}
                                                            </tbody>
                                                        </table>
                                                    </div>
                                                })
                                            } else {
                                                None
                                            }}
                                        }.into_any(),
                                        Err(_) => view! {
                                            <div class="step-metrics-error">
                                                "无法获取 Step 指标数据"
                                            </div>
                                        }.into_any(),
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
