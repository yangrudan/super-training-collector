use leptos::prelude::*;
use crate::api::get_node_stacks;
use crate::models::*;
use crate::components::common::Loading;

/// 堆栈分析面板组件
#[component]
pub fn StackAnalysisPanel(
    #[prop(into)] node_ip: String,
) -> impl IntoView {
    let (show_stacks, set_show_stacks) = signal(false);
    let ip = node_ip.clone();

    let stacks_resource = Resource::new(
        move || (show_stacks.get(), ip.clone()),
        |(should_load, ip)| async move {
            if should_load {
                get_node_stacks(ip).await
            } else {
                Err(ServerFnError::new("Not loaded"))
            }
        },
    );

    view! {
        <section class="stack-analysis">
            <div class="stack-header">
                <h2>"堆栈分析"</h2>
                <button
                    class="collect-btn"
                    on:click=move |_| set_show_stacks.set(true)
                    disabled=move || show_stacks.get()
                >
                    {move || if show_stacks.get() { "已采集" } else { "采集堆栈" }}
                </button>
            </div>

            <Show when=move || show_stacks.get()>
                <Suspense fallback=move || view! { <Loading /> }>
                    {move || {
                        stacks_resource.get().map(|result| {
                            match result {
                                Ok(response) => view! {
                                    <div class="stack-content">
                                        <div class="stack-info">
                                            <span class="stack-meta">
                                                "共 " {response.stacks.len()} " 个 Rank 的堆栈"
                                            </span>
                                        </div>
                                        
                                        // 合并堆栈树视图
                                        <div class="merged-stack-tree">
                                            <h3>"聚合堆栈视图"</h3>
                                            <div class="stack-tree-container">
                                                <StackTreeNode frame=response.merged_root.clone() />
                                            </div>
                                        </div>

                                        // 各 Rank 原始堆栈
                                        <details class="raw-stacks-section">
                                            <summary>"原始堆栈详情"</summary>
                                            <div class="raw-stacks-grid">
                                                {response.stacks.into_iter().map(|stack| {
                                                    view! { <RankStackCard stack=stack /> }
                                                }).collect_view()}
                                            </div>
                                        </details>
                                    </div>
                                }.into_any(),
                                Err(_) => view! {
                                    <div class="stack-placeholder">
                                        "点击「采集堆栈」按钮获取各 Rank 的调用堆栈"
                                    </div>
                                }.into_any(),
                            }
                        })
                    }}
                </Suspense>
            </Show>

            <Show when=move || !show_stacks.get()>
                <div class="stack-placeholder">
                    <p>"点击「采集堆栈」按钮获取各 Rank 的调用堆栈并生成聚合视图"</p>
                    <p class="hint">"堆栈分析可帮助定位慢 Rank 的根因，如 NCCL 等待、IO 阻塞等"</p>
                </div>
            </Show>
        </section>
    }
}

/// 合并堆栈树节点组件
#[component]
fn StackTreeNode(frame: MergedStackFrame) -> impl IntoView {
    let has_children = !frame.children.is_empty();
    let coverage_class = frame.coverage_class();
    let rank_str = frame.rank_range_str();
    let coverage_pct = (frame.coverage() * 100.0) as u32;

    view! {
        <div class=format!("stack-frame {}", coverage_class)>
            <div class="frame-header">
                <span class="frame-name">{frame.frame_name.clone()}</span>
                <span class="frame-stats">
                    <span class="rank-count" title=format!("Ranks: {}", rank_str)>
                        {frame.rank_count} "/" {frame.total_ranks}
                    </span>
                    <span class=format!("coverage-badge {}", coverage_class)>
                        {coverage_pct} "%"
                    </span>
                </span>
            </div>

            {has_children.then(|| {
                view! {
                    <div class="frame-children">
                        {frame.children.into_iter().map(|child| {
                            view! { <StackTreeNode frame=child /> }
                        }).collect_view()}
                    </div>
                }
            })}
        </div>
    }
}

/// 单个 Rank 堆栈卡片
#[component]
fn RankStackCard(stack: RankStack) -> impl IntoView {
    view! {
        <div class="rank-stack-card">
            <div class="rank-stack-header">
                <span class="rank-id">"Rank " {stack.rank_id}</span>
            </div>
            <div class="callstack">
                {stack.callstack.into_iter().enumerate().map(|(i, frame)| {
                    view! {
                        <div class="stack-line">
                            <span class="line-num">{i}</span>
                            <span class="frame-text">{frame}</span>
                        </div>
                    }
                }).collect_view()}
            </div>
        </div>
    }
}
