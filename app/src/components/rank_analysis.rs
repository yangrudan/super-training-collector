//! 问题 Rank 分析组件
//!
//! 提供手动触发分析和展示分析结果的 UI 组件

use crate::rank_analysis_types::{AnalysisTrigger, ProblematicRank, RankAnalysisResult};
use leptos::prelude::*;

/// 问题 Rank 分析面板（完整版，用于 Level2 Tab）
#[component]
pub fn RankAnalysisPanel() -> impl IntoView {
    use crate::api::{analyze_problematic_ranks, get_hang_status, get_problematic_ranks};
    use crate::hang_types::HangStatus;

    let (is_analyzing, set_is_analyzing) = signal(false);
    let analysis_result: RwSignal<Option<RankAnalysisResult>> = RwSignal::new(None);
    let analysis_error: RwSignal<Option<String>> = RwSignal::new(None);
    let hang_status_resource = Resource::new(|| (), |_| get_hang_status());
    let cached_resource = Resource::new(
        move || match hang_status_resource.get() {
            Some(Ok(snapshot)) if snapshot.status == HangStatus::Hang => Some(()),
            _ => None,
        },
        |key| async move {
            match key {
                Some(_) => get_problematic_ranks().await,
                None => Ok(None),
            }
        },
    );

    // 手动触发分析
    let on_analyze = move |_| {
        set_is_analyzing.set(true);
        analysis_error.set(None);
        leptos::task::spawn_local(async move {
            match analyze_problematic_ranks().await {
                Ok(result) => {
                    analysis_result.set(Some(result));
                }
                Err(e) => {
                    analysis_error.set(Some(e.to_string()));
                }
            }
            set_is_analyzing.set(false);
        });
    };

    view! {
        <div class="rank-analysis-panel">
            <Suspense fallback=move || view! {
                <div class="rank-analysis-loading">"检查 HANG 状态中..."</div>
            }>
                {move || {
                    hang_status_resource.get().map(|result| match result {
                        Ok(snapshot) if snapshot.status == HangStatus::Hang => view! {
                            <div class="rank-analysis-gated-content">
                                <div class="rank-analysis-toolbar">
                                    <button
                                        class="collect-btn rank-analysis-btn"
                                        on:click=on_analyze
                                        disabled=move || is_analyzing.get()
                                    >
                                        {move || if is_analyzing.get() {
                                            "🔍 分析中..."
                                        } else {
                                            "🔍 分析问题 Rank"
                                        }}
                                    </button>
                                    <span class="rank-analysis-hint">
                                        "已检测到 HANG，可分析分叉异常"
                                    </span>
                                </div>

                                <Show when=move || analysis_error.get().is_some()>
                                    <div class="rank-analysis-error">
                                        <span>"❌ "</span>
                                        {move || analysis_error.get().unwrap_or_default()}
                                    </div>
                                </Show>

                                {move || {
                                    if let Some(result) = analysis_result.get() {
                                        return Some(
                                            view! { <RankAnalysisResultView result=result /> }.into_any(),
                                        );
                                    }
                                    None
                                }}

                                <Show when=move || analysis_result.get().is_none()>
                                    <Suspense fallback=move || view! {
                                        <div class="rank-analysis-loading">"加载缓存结果..."</div>
                                    }>
                                        {move || {
                                            cached_resource.get().map(|result| {
                                                match result {
                                                    Ok(Some(cached)) => {
                                                        view! { <RankAnalysisResultView result=cached /> }.into_any()
                                                    }
                                                    Ok(None) => {
                                                        view! {
                                                            <div class="rank-analysis-empty">
                                                                <span class="empty-icon">"📊"</span>
                                                                <p>"暂无分析结果"</p>
                                                                <p class="empty-hint">"点击上方按钮手动触发分析，或等待 HANG 检测自动触发"</p>
                                                            </div>
                                                        }.into_any()
                                                    }
                                                    Err(_) => {
                                                        view! {
                                                            <div class="rank-analysis-empty">
                                                                <p>"无法获取缓存的分析结果"</p>
                                                            </div>
                                                        }.into_any()
                                                    }
                                                }
                                            })
                                        }}
                                    </Suspense>
                                </Show>
                            </div>
                        }
                        .into_any(),
                        Ok(snapshot) => {
                            let hint = match snapshot.status {
                                HangStatus::Normal => "当前未检测到 HANG，暂不执行问题 Rank 分析",
                                HangStatus::Disabled => "HANG 检测未启用，暂不执行问题 Rank 分析",
                                HangStatus::Hang => unreachable!(),
                            };

                            view! {
                                <div class="rank-analysis-empty">
                                    <span class="empty-icon">"🟢"</span>
                                    <p>{hint}</p>
                                    <p class="empty-hint">"问题 Rank 分析只在检测到 HANG 后开启"</p>
                                </div>
                            }
                            .into_any()
                        }
                        Err(_) => view! {
                            <div class="rank-analysis-empty">
                                <p>"无法获取 HANG 状态"</p>
                            </div>
                        }
                        .into_any(),
                    })
                }}
            </Suspense>
        </div>
    }
}

/// 分析结果展示组件
#[component]
fn RankAnalysisResultView(result: RankAnalysisResult) -> impl IntoView {
    let total_ranks = result.total_ranks;
    let problematic_count = result.problematic_ranks.len();
    let duration_ms = result.analysis_duration_ms;
    let threshold = result.minority_threshold;
    let trigger = result.trigger.clone();
    let timestamp = result.timestamp;
    let ranks = result.problematic_ranks.clone();

    let trigger_label = match &trigger {
        AnalysisTrigger::HangDetected => "HANG 自动触发",
        AnalysisTrigger::Manual => "手动触发",
    };

    let severity_class = if problematic_count == 0 {
        "result-normal"
    } else if problematic_count <= 2 {
        "result-warning"
    } else {
        "result-critical"
    };

    view! {
        <div class=format!("rank-analysis-result {}", severity_class)>
            // 概要信息
            <div class="result-summary">
                <div class="result-summary-main">
                    <span class="result-icon">
                        {if problematic_count == 0 { "✅" } else { "⚠️" }}
                    </span>
                    <span class="result-text">
                        {if problematic_count == 0 {
                            format!("所有 {} 个 Rank 运行正常，未检测到分叉异常", total_ranks)
                        } else {
                            format!("检测到 {} 个问题 Rank（共 {} 个）", problematic_count, total_ranks)
                        }}
                    </span>
                </div>
                <div class="result-meta">
                    <span class="meta-item">"触发: " {trigger_label}</span>
                    <span class="meta-item">"耗时: " {duration_ms} " ms"</span>
                    <span class="meta-item">"阈值: " {format!("{:.0}%", threshold * 100.0)}</span>
                    <span class="meta-item">"时间: " {format_timestamp(timestamp)}</span>
                </div>
            </div>

            // 问题 Rank 详情表格
            {if !ranks.is_empty() {
                let ranks_for_table = ranks.clone();
                Some(view! {
                    <div class="rank-table-container">
                        <table class="rank-table">
                            <thead>
                                <tr>
                                    <th>"Rank ID"</th>
                                    <th>"节点 IP"</th>
                                    <th>"异常分数"</th>
                                    <th>"分叉点数"</th>
                                    <th>"主要分叉位置"</th>
                                </tr>
                            </thead>
                            <tbody>
                                {ranks_for_table.into_iter().map(|rank| {
                                    view! { <ProblematicRankRow rank=rank /> }
                                }).collect_view()}
                            </tbody>
                        </table>
                    </div>
                })
            } else {
                None
            }}
        </div>
    }
}

/// 问题 Rank 表格行
#[component]
fn ProblematicRankRow(rank: ProblematicRank) -> impl IntoView {
    let score = rank.anomaly_score;
    let score_class = if rank.issue_reason.is_some() || score >= 3 {
        "score-critical"
    } else if score >= 2 {
        "score-warning"
    } else {
        "score-low"
    };

    let divergence_count = rank.divergence_points.len();

    // 展示最显著的分叉位置（按覆盖率升序，取前 2 个）
    let mut sorted_points = rank.divergence_points.clone();
    sorted_points.sort_by(|a, b| {
        a.minority_coverage
            .partial_cmp(&b.minority_coverage)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let points_display = if let Some(reason) = rank.issue_reason.clone() {
        reason
    } else {
        let top_points: Vec<String> = sorted_points
            .iter()
            .take(2)
            .map(|p| {
                let short_name = shorten_frame_name(&p.frame_name);
                format!("{} ({:.0}%)", short_name, p.minority_coverage * 100.0)
            })
            .collect();
        top_points.join(", ")
    };
    let points_display_title = points_display.clone();
    let score_display = rank
        .issue_reason
        .as_ref()
        .map(|_| "采集异常".to_string())
        .unwrap_or_else(|| score.to_string());

    view! {
        <tr class="rank-row">
            <td class="rank-id">
                <span class="rank-id-badge">{format!("Rank {}", rank.rank_id)}</span>
            </td>
            <td>{rank.node_ip.clone().unwrap_or_else(|| "-".to_string())}</td>
            <td>
                <span class=format!("anomaly-score {}", score_class)>{score_display}</span>
            </td>
            <td>{divergence_count}</td>
            <td class="divergence-info" title=points_display_title>
                {points_display}
            </td>
        </tr>
    }
}

/// 紧凑版分析结果摘要（用于 Level1 页面）
#[component]
pub fn RankAnalysisSummary() -> impl IntoView {
    use crate::api::get_problematic_ranks;

    let (refresh_trigger, set_refresh_trigger) = signal(0u32);

    #[cfg(not(feature = "ssr"))]
    {
        use leptos::task::spawn_local;
        spawn_local(async move {
            loop {
                gloo_timers::future::TimeoutFuture::new(30_000).await;
                set_refresh_trigger.update(|n| *n = n.wrapping_add(1));
            }
        });
    }

    #[cfg(feature = "ssr")]
    let _ = set_refresh_trigger;

    let cached_resource = Resource::new(move || refresh_trigger.get(), |_| get_problematic_ranks());

    view! {
        <Suspense fallback=|| ()>
            {move || {
                cached_resource.get().and_then(|result| {
                    match result {
                        Ok(Some(analysis)) if !analysis.problematic_ranks.is_empty() => {
                            let count = analysis.problematic_ranks.len();
                            let total = analysis.total_ranks;
                            let top_rank = analysis.problematic_ranks.first()
                                .map(|r| {
                                    r.issue_reason
                                        .as_ref()
                                        .map(|reason| format!("Rank {} ({})", r.rank_id, reason))
                                        .unwrap_or_else(|| format!("Rank {} (分数: {})", r.rank_id, r.anomaly_score))
                                })
                                .unwrap_or_default();

                            Some(view! {
                                <section class="rank-analysis-summary-section">
                                    <h2>"问题 Rank 检测"</h2>
                                    <div class="rank-summary-card">
                                        <div class="rank-summary-header">
                                            <span class="rank-summary-icon">"⚠️"</span>
                                            <span class="rank-summary-text">
                                                {format!("检测到 {} 个问题 Rank（共 {} 个）", count, total)}
                                            </span>
                                        </div>
                                        <div class="rank-summary-detail">
                                            <span>"最异常: " {top_rank}</span>
                                        </div>
                                        <a href="/nodes" class="rank-summary-link">"查看详情 →"</a>
                                    </div>
                                </section>
                            }.into_any())
                        }
                        _ => None,
                    }
                })
            }}
        </Suspense>
    }
}

/// 缩短函数名显示
fn shorten_frame_name(name: &str) -> String {
    // 取最后一个 :: 之后的部分，如果总长度超过 40 则截断
    let short = name.rsplit("::").next().unwrap_or(name);
    if short.len() > 40 {
        format!("{}...", &short[..37])
    } else {
        short.to_string()
    }
}

/// 格式化 Unix 时间戳
fn format_timestamp(ts: u64) -> String {
    if ts == 0 {
        return "-".to_string();
    }
    // 简单格式化：显示相对时间或绝对时间
    // 在 WASM 环境中不方便使用 chrono，用简单格式
    format!("timestamp:{}", ts)
}
