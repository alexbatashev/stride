use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::HeaderMap,
    response::{Html, IntoResponse, Redirect, Response},
};
use uuid::Uuid;

use crate::components::{
    side_panel::Stores as SidePanelStores,
    thread_view::Stores as ThreadViewStores,
    threads_page_view::{RenderStores, ThreadsPageView, ThreadsPageViewServer},
    ui::Stores as UiStores,
};
use crate::{ServerState, api::threads};

struct ThreadPageServer {
    state: Arc<ServerState>,
    headers: HeaderMap,
}

impl ThreadsPageViewServer for ThreadPageServer {
    type Error = threads::ThreadApiError;

    async fn load_thread_page(
        &self,
        thread_id: &str,
    ) -> Result<crate::components::threads_page_view::ThreadPageData, Self::Error> {
        let thread_id = if thread_id.is_empty() {
            None
        } else {
            Some(
                thread_id
                    .parse()
                    .map_err(|_| threads::ThreadApiError::NotFound)?,
            )
        };
        threads::thread_page_data(&self.state, &self.headers, thread_id)
            .await
            .map(super::argon_thread_page_data)
    }
}

pub async fn new_thread(State(state): State<Arc<ServerState>>, headers: HeaderMap) -> Response {
    render_threads(state, headers, None).await
}

pub async fn thread(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Response {
    render_threads(state, headers, Some(id)).await
}

async fn render_threads(
    state: Arc<ServerState>,
    headers: HeaderMap,
    thread_id: Option<Uuid>,
) -> Response {
    let thread_id = thread_id.map(|id| id.to_string()).unwrap_or_default();
    let mut ui_stores = UiStores::default();
    ui_stores.sidebar.active_thread = thread_id.clone();
    let thread_view_stores = ThreadViewStores::default();
    let side_panel_stores = SidePanelStores::default();
    let stores = RenderStores {
        side_panel: &side_panel_stores,
        thread_view: &thread_view_stores,
        ui: &ui_stores,
    };
    let server = ThreadPageServer { state, headers };
    let page = ThreadsPageView::new(&thread_id)
        .attr("id", "threads-page")
        .attr("data-thread-id", &thread_id);
    let store_payload = super::combine_store_snapshots(&[
        ui_stores.snapshot_json(),
        thread_view_stores.snapshot_json(),
        side_panel_stores.snapshot_json(),
    ]);
    let opts = super::argon_document_opts("S.T.R.I.D.E.", &store_payload);

    match page.render_document(&server, &stores, &opts).await {
        Ok(html) => Html(html).into_response(),
        Err(threads::ThreadApiError::Auth(_)) => Redirect::to("/auth/login").into_response(),
        Err(error) => error.into_response(),
    }
}
