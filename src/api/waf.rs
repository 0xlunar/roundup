use anyhow::format_err;
use html_executor::{render_html, DriverCapability, RenderOptions, RenderResults};
use log::{info, warn};
use reqwest::cookie::Jar;
use reqwest::header::{HeaderMap, HeaderValue};
use reqwest::{Client, ClientBuilder, Proxy, RequestBuilder, Response};
use std::ops::Deref;
use std::sync::Arc;
use std::time::Duration;

const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/147.0.0.0 Safari/537.36";

#[async_trait::async_trait]
pub trait SendCaptchaHandler {
    async fn send_and_handle_waf(
        self,
        webdriver_url: Option<Arc<str>>,
        jar: Arc<Jar>,
    ) -> anyhow::Result<Response>;
}

#[async_trait::async_trait]
impl SendCaptchaHandler for RequestBuilder {
    async fn send_and_handle_waf(
        self,
        webdriver_url: Option<Arc<str>>,
        jar: Arc<Jar>,
    ) -> anyhow::Result<Response> {
        let original_request = match self.try_clone() {
            Some(request) => request,
            None => return Err(format_err!("Unable to handle waf with this request")),
        };

        // Send initial request
        let response = self.send().await?;

        // Check if challenge header exists and we got a 202 accepted status
        // let challenge_header = response
        //     .headers()
        //     .get("x-amzn-waf-action")
        //     .filter(|header| header.as_bytes() == "challenge".as_bytes())
        //     .is_some();
        if response.status().as_u16() != 202 {
            return Ok(response);
        }

        let webdriver_url = if let Some(webdriver_url) = webdriver_url {
            webdriver_url
        } else {
            return Ok(response);
        };

        let url = response.url().clone();
        warn!("WAF Detected! - {}", url.as_str());
        let render_options = RenderOptions {
            html: None,
            url: url.as_str(),
            driver_url: Some(&*webdriver_url),
            output_delay: Some(Duration::from_secs(8)),
            driver_capability: DriverCapability::Chrome,
            user_agent: Some(USER_AGENT),
            headless: true,
            cookie_only: true,
        };

        let RenderResults { cookies, .. } = render_html(render_options).await?;

        // Add Cookies from rendering result to client
        let mut cookie_header = vec![];
        for cookie in cookies {
            let cookie = format!("{}={}", cookie.name, cookie.value);
            jar.add_cookie_str(&cookie, &url);
            cookie_header.push(cookie);
        }

        // let cookie_header = cookie_header.join("; ");
        // send original request with updated cookies?
        Ok(original_request
            // .header("cookie", cookie_header)
            .send()
            .await?)
    }
}

#[derive(Debug, Clone, Default)]
pub struct AwsWafClient {
    client: Arc<Client>,
    jar: Arc<Jar>,
    webdriver_url: Option<Arc<str>>,
}

impl AwsWafClient {
    pub fn new(webdriver_url: Option<Arc<str>>, proxy: Option<Proxy>) -> Self {
        let mut headers = HeaderMap::new();
        headers.append(
            "accept",
            HeaderValue::from_static(
                "ext/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,image/apng,*/*;q=0.8,application/signed-exchange;v=b3;q=0.7",
            ),
        );
        headers.append(
            "accept-encoding",
            HeaderValue::from_static("gzip, deflate, br, zstd"),
        );
        headers.append(
            "accept-language",
            HeaderValue::from_static("en-US,en;q=0.9"),
        );
        headers.append("connection", HeaderValue::from_static("keep-alive"));
        headers.append("cache-control", HeaderValue::from_static("no-cache"));
        headers.append("pragma", HeaderValue::from_static("no-cache"));
        headers.append("priority", HeaderValue::from_static("u=0, i"));
        headers.append(
            "sec-ch-ua",
            HeaderValue::from_static(
                r#""Google Chrome";v="147", "Not.A/Brand";v="8", "Chromium";v="147""#,
            ),
        );
        headers.append("sec-ch-ua-mobile", HeaderValue::from_static("?0"));
        headers.append(
            "sec-ch-ua-platform",
            HeaderValue::from_static(r#""Windows""#),
        );
        headers.append("sec-fetch-dest", HeaderValue::from_static("document"));
        headers.append("sec-fetch-mode", HeaderValue::from_static("navigate"));
        headers.append("sec-fetch-site", HeaderValue::from_static("none"));
        headers.append("sec-fetch-user", HeaderValue::from_static("?1"));
        headers.append("upgrade-insecure-requests", HeaderValue::from_static("1"));

        let jar = Arc::new(Jar::default());
        let client = ClientBuilder::new()
            .cookie_provider(jar.clone())
            .user_agent(USER_AGENT)
            .gzip(true)
            .deflate(true)
            .brotli(true)
            .zstd(true)
            .default_headers(headers);

        let client = match proxy {
            Some(p) => client.proxy(p.to_owned()),
            None => client,
        }
        .build()
        .unwrap();

        let client = Arc::new(client);

        Self {
            client,
            jar,
            webdriver_url,
        }
    }

    pub fn jar(&self) -> Arc<Jar> {
        self.jar.clone()
    }

    pub fn webdriver(&self) -> Option<Arc<str>> {
        self.webdriver_url.clone()
    }
}

impl Deref for AwsWafClient {
    type Target = Client;

    fn deref(&self) -> &Self::Target {
        &self.client
    }
}

async fn test() {
    let capmonster = AwsWafClient::default();
    capmonster
        .get("")
        .send_and_handle_waf(capmonster.webdriver(), capmonster.jar())
        .await
        .unwrap();
}
