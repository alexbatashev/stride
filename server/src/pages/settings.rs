use axum::response::Redirect;

pub async fn settings() -> Redirect {
    Redirect::temporary("/threads?settings=open")
}

#[cfg(test)]
mod tests {
    use axum::{http::header, response::IntoResponse};

    #[tokio::test]
    async fn legacy_settings_route_opens_the_dialog_on_threads() {
        let response = super::settings().await.into_response();
        assert_eq!(
            response.status(),
            axum::http::StatusCode::TEMPORARY_REDIRECT
        );
        assert_eq!(
            response.headers().get(header::LOCATION).unwrap(),
            "/threads?settings=open"
        );
    }
}
