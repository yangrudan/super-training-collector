use leptos::prelude::*;
use crate::api::get_global_metrics;
use crate::components::common::*;

/// Level 1: 全局态势视图
#[component]
pub fn Level1View() -> impl IntoView {
    let (refresh_trigger, set_refresh_trigger) = signal(0u32);
    
    let global_resource = Resource::new(
        move || refresh_trigger.get(),
        |_| get_global_metrics()
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

                                    // 训练进度区域
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
