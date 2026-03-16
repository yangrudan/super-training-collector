use crate::api::get_node_flamegraph;
use crate::components::common::Loading;
use leptos::prelude::*;

/// 堆栈分析面板组件
#[component]
pub fn StackAnalysisPanel(#[prop(into)] node_ip: String) -> impl IntoView {
    let (loading, set_loading) = signal(false);
    let flamegraph_svg: RwSignal<Option<String>> = RwSignal::new(None);
    let error_msg: RwSignal<Option<String>> = RwSignal::new(None);

    let ip = node_ip.clone();
    let on_generate = move |_| {
        let ip = ip.clone();
        set_loading.set(true);
        flamegraph_svg.set(None);
        error_msg.set(None);
        leptos::task::spawn_local(async move {
            match get_node_flamegraph(ip).await {
                Ok(svg) => flamegraph_svg.set(Some(svg)),
                Err(e) => error_msg.set(Some(e.to_string())),
            }
            set_loading.set(false);
        });
    };

    view! {
        <section class="stack-analysis">
            <div class="stack-header">
                <h2>"堆栈分析"</h2>
                <button
                    class="collect-btn"
                    on:click=on_generate
                    disabled=move || loading.get()
                >
                    {move || if loading.get() { "采集中..." } else { "采集堆栈" }}
                </button>
            </div>

            <Show when=move || loading.get()>
                <Loading />
            </Show>

            <Show when=move || error_msg.get().is_some()>
                <div class="stack-error">
                    {move || error_msg.get().unwrap_or_default()}
                </div>
            </Show>

            <Show when=move || flamegraph_svg.get().is_some()>
                <div class="flamegraph-container">
                    <div
                        class="flamegraph-svg"
                        inner_html=move || flamegraph_svg.get().unwrap_or_default()
                    />
                </div>
            </Show>

            <Show when=move || !loading.get() && flamegraph_svg.get().is_none() && error_msg.get().is_none()>
                <div class="stack-placeholder">
                    <p>"点击「采集堆栈」采集各 Rank 调用栈并生成火焰图"</p>
                    <p class="hint">"可帮助定位慢 Rank 的根因，如 NCCL 等待、IO 阻塞等"</p>
                </div>
            </Show>
        </section>
    }
}
