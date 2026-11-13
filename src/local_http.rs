use std::time::Duration;

/// LAN controllers must not inherit a host proxy or redirect credentials elsewhere.
pub(crate) fn agent(timeout: Option<Duration>) -> ureq::Agent {
    ureq::Agent::config_builder()
        .proxy(None)
        .max_redirects(0)
        .timeout_global(timeout)
        .build()
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lan_requests_ignore_environment_proxies_and_redirects() {
        if std::env::var_os("LG_HUE_SYNC_PROXY_TEST_CHILD").is_none() {
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "local_http::tests::lan_requests_ignore_environment_proxies_and_redirects",
                ])
                .env("LG_HUE_SYNC_PROXY_TEST_CHILD", "1")
                .env("HTTP_PROXY", "http://127.0.0.1:9")
                .env("HTTPS_PROXY", "http://127.0.0.1:9")
                .env("ALL_PROXY", "http://127.0.0.1:9")
                .status()
                .unwrap();
            assert!(status.success());
            return;
        }
        let agent = agent(None);
        assert!(agent.config().proxy().is_none());
        assert_eq!(agent.config().max_redirects(), 0);
    }
}
