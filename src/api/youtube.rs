use anyhow::format_err;
use reqwest::{Client, ClientBuilder};
use serde::Deserialize;

pub struct Youtube {
    client: Client,
    api_key: String,
}

impl Youtube {
    pub fn new(api_key: &str) -> Self {
        let client = ClientBuilder::new().user_agent("roundup/1.0").build().unwrap();

        Youtube {
            client,
            api_key: api_key.to_string()
        }
    }

    pub async fn search(&self, query: &str) -> anyhow::Result<Vec<(String, String)>> {
        let query = urlencoding::encode(query);
        let query_params = vec![
            ("part","snippet"),
            ("q", query.as_ref()),
            ("key", &self.api_key),
        ];

        let resp = self.client.get("https://www.googleapis.com/youtube/v3/search").query(&query_params).send().await?;
        if resp.status().is_server_error() || resp.status().is_client_error() {
            let status = resp.status();
            let text = resp.text().await?;
            return Err(format_err!("Failed to send request, Status: {}, Body: {}", status, text))
        }

        let text = resp.text().await?;
        let data: YoutubeSearchResponse = serde_json::from_str(&text)?;

        let data = data.items.into_iter().map(|x| (x.snippet.title, x.id.video_id)).collect::<Vec<(String, String)>>();

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
    snippet: YoutubeSearchSnippet
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