use leptos::prelude::*;
use leptos_meta::{provide_meta_context, MetaTags, Stylesheet, Title};
use leptos_router::{
    components::{Route, Router, Routes},
    StaticSegment, ParamSegment,
};

pub mod models;
pub mod mock;
pub mod api;
pub mod components;
pub mod adapter;
#[cfg(feature = "ssr")]
pub mod flamegraph;

use components::{Level1View, Level2View, Level3View};

pub fn shell(options: LeptosOptions) -> impl IntoView {
    view! {
        <!DOCTYPE html>
        <html lang="zh-CN">
            <head>
                <meta charset="utf-8"/>
                <meta name="viewport" content="width=device-width, initial-scale=1"/>
                <AutoReload options=options.clone()/>
                <HydrationScripts options/>
                <MetaTags/>
            </head>
            <body>
                <App/>
            </body>
        </html>
    }
}

#[component]
pub fn App() -> impl IntoView {
    provide_meta_context();

    view! {
        <Stylesheet id="leptos" href="/pkg/super-trainning-collector.css"/>
        <Title text="训练任务监控面板"/>

        <Router>
            <main class="app-container">
                <Routes fallback=|| "页面未找到".into_view()>
                    <Route path=StaticSegment("") view=Level1View/>
                    <Route path=StaticSegment("nodes") view=Level2View/>
                    <Route path=(StaticSegment("nodes"), ParamSegment("ip")) view=Level3View/>
                </Routes>
            </main>
        </Router>
    }
}
