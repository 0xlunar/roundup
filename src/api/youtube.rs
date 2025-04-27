use anyhow::format_err;
use rayon::prelude::*;
use rquest::Client;
use serde::Deserialize;

pub struct Youtube {
    client: Client,
    api_key: String,
}

impl Youtube {
    pub fn new(api_key: &str, proxy: Option<&String>) -> Self {
        let user_agent = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));
        let mut client = rquest::ClientBuilder::new()
            .user_agent(user_agent)
            .zstd(true)
            .brotli(true)
            .deflate(true)
            .gzip(true);
        client = match proxy {
            Some(p) => client.proxy(p.to_owned()),
            None => client,
        };

        let client = client.build().unwrap();

        Youtube {
            client,
            api_key: api_key.to_string(),
        }
    }

    pub async fn search(&self, query: &str) -> anyhow::Result<Vec<(String, String)>> {
        let query = urlencoding::encode(query);
        let query_params = vec![
            ("part", "snippet"),
            ("q", query.as_ref()),
            ("key", &self.api_key),
        ];

        let resp = self
            .client
            .get("https://www.googleapis.com/youtube/v3/search")
            .query(&query_params)
            .send()
            .await?;
        if resp.status().is_server_error() || resp.status().is_client_error() {
            let status = resp.status();
            let text = resp.text().await?;
            return Err(format_err!(
                "Failed to send request, Status: {}, Body: {}",
                status,
                text
            ));
        }

        let text = resp.text().await?;
        let data: YoutubeSearchResponse = serde_json::from_str(&text)?;

        let data = data
            .items
            .into_par_iter()
            .map(|x| (x.snippet.title, x.id.video_id))
            .collect::<Vec<(String, String)>>();

        Ok(data)
    }
}

#[derive(Deserialize, Clone)]
struct YoutubeSearchResponse {
    items: Vec<YoutubeSearchItem>,
}
#[derive(Deserialize, Clone)]
struct YoutubeSearchItem {
    id: YoutubeSearchItemId,
    snippet: YoutubeSearchSnippet,
}
#[derive(Deserialize, Clone)]
struct YoutubeSearchItemId {
    #[serde(rename = "videoId")]
    video_id: String,
}

#[derive(Deserialize, Clone)]
struct YoutubeSearchSnippet {
    title: String,
}
