pub mod downloader;

pub use downloader::ReqwestDownloader;

/// Asks each web address whether it answers, without downloading it: a HEAD
/// request, or a GET whose body is not read when HEAD is refused. Only http
/// and https addresses are tried; nothing is followed beyond a few redirects.
pub async fn check_links(urls: &[String]) -> Vec<manager_app::api::dto::LinkCheckDto> {
    let client = reqwest::Client::builder()
        .user_agent("StardewModManager/0.1.0")
        .timeout(std::time::Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());
    let mut out = Vec::new();
    for url in urls {
        let lower = url.to_lowercase();
        let checked_at = chrono::Utc::now().to_rfc3339();
        if !(lower.starts_with("https://") || lower.starts_with("http://")) {
            out.push(manager_app::api::dto::LinkCheckDto {
                url: url.clone(),
                state: "not_checked".into(),
                status: None,
                checked_at,
            });
            continue;
        }
        let mut response = client.head(url).send().await;
        if let Ok(r) = &response {
            if r.status() == reqwest::StatusCode::METHOD_NOT_ALLOWED {
                response = client.get(url).send().await;
            }
        }
        let (state, status) = match response {
            Ok(r) => {
                let code = r.status();
                let state = if code.is_success() {
                    "reachable"
                } else if code == reqwest::StatusCode::NOT_FOUND
                    || code == reqwest::StatusCode::GONE
                {
                    "not_found"
                } else {
                    "error"
                };
                (state, Some(code.as_u16()))
            }
            Err(_) => ("unreachable", None),
        };
        out.push(manager_app::api::dto::LinkCheckDto {
            url: url.clone(),
            state: state.into(),
            status,
            checked_at,
        });
    }
    out
}

#[cfg(test)]
mod link_tests {
    #[tokio::test]
    async fn addresses_that_are_not_web_links_are_not_tried() {
        let out = super::check_links(&["ftp://x".into(), "javascript:alert(1)".into()]).await;
        assert!(out.iter().all(|c| c.state == "not_checked"));
    }
}
