use std::ops::{Deref, Div, Not};

use actix_web::{Error, get, HttpResponse, web};
use actix_web::error::{ErrorBadRequest, ErrorInternalServerError};
use actix_web::web::{Data, Query};
use anyhow::format_err;
use chrono::{Duration, Local};
use log::error;
use rayon::prelude::*;
use serde::Deserialize;
use tokio::sync::Mutex;

use crate::api::imdb::{IMDB, IMDBItem, ItemType, SearchType};
use crate::api::youtube::Youtube;
use crate::db::DBConnection;
use crate::db::downloads::{ActiveDownloadIMDBItem, DownloadDatabase};
use crate::db::imdb::IMDBDatabase;
use crate::QueryCache;

#[derive(Deserialize)]
pub struct SearchQueryParams {
    #[serde(rename = "type")]
    _type: String,
    mode: String,
    query: Option<String>,
}
#[get("/search")]
pub async fn search(
    params: Query<SearchQueryParams>,
    cache_update: web::Data<Mutex<QueryCache>>,
    db: web::Data<DBConnection>,
) -> Result<HttpResponse<String>, Error> {
    let _type = match params._type.to_ascii_lowercase().as_str() {
        "movie" | "film" => ItemType::Movie,
        "tv" | "show" | "series" => ItemType::TvShow,
        _ => ItemType::Movie,
    };

    let mode = match params.mode.to_ascii_lowercase().as_str() {
        "latest" | "recent" | "release" => match _type {
            ItemType::Movie => SearchType::MovieLatestRelease,
            ItemType::TvShow => SearchType::TVLatestRelease,
        },
        "popular" | "trending" => match _type {
            ItemType::Movie => SearchType::MoviePopular,
            ItemType::TvShow => SearchType::TVPopular,
        },
        "query" => {
            match &params.query {
                Some(t) => {
                    if t.is_empty() {
                        SearchType::MoviePopular // for when the text box is emptied, show the default page
                    } else {
                        SearchType::Query(t.to_owned())
                    }
                }
                None => return Err(ErrorBadRequest("Missing Query Field")),
            }
        }
        "watchlist" => SearchType::Watchlist,
        "downloads" => SearchType::Downloads,
        _ => match _type {
            ItemType::Movie => SearchType::MoviePopular,
            ItemType::TvShow => SearchType::TVPopular,
        },
    };

    if mode == SearchType::Downloads {
        let db = DownloadDatabase::new(&db);
        let items = match db.fetch_downloads_with_imdb_data().await {
            Ok(t) => t,
            Err(e) => return Err(ErrorInternalServerError(e)),
        };

        let html = generate_active_downloads_html(items);
        return Ok(HttpResponse::Ok().message_body(html).unwrap());
    }

    let results = check_cache_then_search_imdb(mode, db, cache_update).await?;

    let html = generate_search_html_imdb(results);
    Ok(HttpResponse::Ok().message_body(html).unwrap())
}

fn generate_active_downloads_html(items: Vec<ActiveDownloadIMDBItem>) -> String {
    let mut output = String::new();

    output.push_str(
        "<div style=\"display: flex; flex-direction: row; align-items: center; flex-wrap: wrap;\">",
    );
    let items = generate_active_downloads_items(items);
    output.push_str(&items);
    output.push_str("</div>");
    output
}

fn generate_active_downloads_items(items: Vec<ActiveDownloadIMDBItem>) -> String {
    let mut output = String::new();

    for item in items {
        output.push_str(
            "<div class=\"card\" style=\"min-width: 24rem; width: 24rem; max-height: 14rem; margin: 0.5rem;\">",
        );
        output.push_str("<div class=\"row\">");
        output.push_str("<div class=\"col\">");
        let image = format!(
            "<img src=\"{}\" alt=\"imdb_image\" style=\"max-height: 14rem;\"/>",
            item.image_url
        );
        output.push_str(&image);
        output.push_str("</div>");

        output.push_str("<div class=\"col\">");
        output.push_str("<div class=\"card-body\">");
        let title = format!("<h5 class=\"card-title\">{}</h5>", item.title);
        output.push_str(&title);

        let subheading = format!("{} | {}", item.year, &item.rating);

        let season_episode_text = match item.season {
            Some(t) => {
                let episode = match item.episode {
                    Some(t) => format!(" | Episode: <b>{}</b>", t),
                    None => String::new(),
                };
                format!("<p>Season: <b>{}</b>{}</p>", t, episode)
            }
            None => String::new(),
        };

        let heading = format!(
            "<div class=\"card-text\">\
    <p><small>{}</small></p>\
    {}\
    <p>{} | {:.2}%</p>\
    </div>",
            subheading,
            season_episode_text,
            item.state,
            item.progress * 100.00
        );
        output.push_str(&heading);
        output.push_str("</div>");
        output.push_str("</div>");
        output.push_str("</div>");
        output.push_str("</div>");
    }

    output
}

// IMDB FUNCTIONS
async fn check_cache_then_search_imdb(
    search_type: SearchType,
    db: web::Data<DBConnection>,
    cache_update: web::Data<Mutex<QueryCache>>,
) -> Result<Vec<IMDBItem>, Error> {
    let imdb_db = IMDBDatabase::new(db.deref());
    let mut twelve_hour_ago: chrono::DateTime<Local> = Local::now();
    twelve_hour_ago = twelve_hour_ago
        .checked_sub_signed(Duration::hours(12))
        .unwrap();

    let updated_at = match cache_update
        .lock()
        .await
        .par_iter_mut()
        .find_any(|(s_t, _)| s_t == &search_type)
    {
        Some((_, d_t)) => {
            let last_updated = d_t.to_owned();
            *d_t = Local::now();
            last_updated
        }
        None => twelve_hour_ago.to_owned(),
    };
    drop(cache_update);

    let mut output: Vec<IMDBItem> = match imdb_db.fetch(search_type.to_owned()).await {
        Ok(t) => t,
        Err(_) => return Err(ErrorInternalServerError("Failed to fetch from cache")),
    };

    if updated_at.le(&twelve_hour_ago) || output.is_empty() {
        match &search_type {
            SearchType::Query(_) | SearchType::Watchlist => {
                if let Ok(a) = imdb_db.fetch(search_type.to_owned()).await {
                    if a.is_empty().not() {
                        return Ok(a);
                    }
                }
            }
            _ => (),
        };

        // Grab new results
        let imdb: IMDB = IMDB::new(search_type, None);
        let items = match imdb.search().await {
            Ok(i) => i,
            Err(e) => return Err(ErrorInternalServerError(e)),
        };

        // Update DB
        match imdb_db.insert_or_update_many(&items).await {
            Ok(_) => (),
            Err(_) => {
                return Err(ErrorInternalServerError(
                    "Failed to insert or update IMDB items",
                ))
            }
        };
        output = items;
    };

    Ok(output)
}

fn generate_search_html_imdb(results: Vec<IMDBItem>) -> String {
    let items = results
        .par_iter()
        .map(generate_item_html_imdb)
        .collect::<Vec<String>>()
        .join("");

    format!("<div class=\"results-container\">{}</div>", items)
}

fn generate_item_html_imdb(item: &IMDBItem) -> String {
    let _type = match item._type {
        ItemType::Movie => "movie",
        ItemType::TvShow => "tv",
    };

    format!("<div id=\"{}\" onclick=\"htmx.trigger('.htmx-request', 'htmx:abort')\" class=\"card\" style=\"width: 8rem; cursor: pointer;\" hx-get=\"/modal_metadata?id={}\" hx-target=\"#download-select\" hx-swap=\"outerHTML\" hx-indicator=\"#download-select\" hx-sync=\"#download-select:replace\" data-bs-toggle=\"modal\" data-bs-target=\"#download-modal\">\
                <img src={} alt=\"media-image\" hx-trigger=\"intersect once\"/>\
                <div class=\"card-body\">\
                    <p class=\"card-text\">{} ({})</p>\
                </div>\
            </div>", &item.id, &item.id, item.image_url, item.title, item.year)
}

#[derive(Deserialize)]
struct ModalMetadataQuery {
    id: String,
}

#[get("/modal_metadata")]
pub async fn modal_metadata(
    params: Query<ModalMetadataQuery>,
    db: web::Data<DBConnection>,
    yt: web::Data<Youtube>,
) -> Result<HttpResponse<String>, Error> {
    let body = {
        let mut cached_item = match get_cached_item_imdb(&params.id, Data::clone(&db)).await {
            Ok(t) => t,
            Err(e) => return Err(ErrorInternalServerError(e)),
        };

        let mut made_changes = false;

        if cached_item.video_url.is_none() {
            let query = format!("{} ({}) Trailer", cached_item.title, cached_item.year);
            let video_url = match yt.search(&query).await {
                Ok(t) => t
                    .par_iter()
                    .find_first(|(title, _)| {
                        let title = title.to_lowercase();
                        let cache_title = cached_item.title.to_lowercase();
                        title.contains(&cache_title) && title.contains("trailer")
                    })
                    .map(|(_, id)| id.to_string()),
                Err(e) => {
                    error!("{}", e);
                    None
                }
            };
            if video_url.is_some() {
                made_changes = true;
            }

            cached_item.video_url = video_url;
        }

        if cached_item.plot.is_none() {
            let metadata = match IMDB::update_media_data(&params.id, None, None).await {
                Ok(t) => t,
                Err(e) => return Err(ErrorInternalServerError(e)),
            };

            if metadata.plot.is_some() {
                made_changes = true;
            }

            cached_item.plot = metadata.plot;
            cached_item.rating = metadata.rating;
            cached_item.runtime = metadata.runtime;
        }

        if made_changes {
            let imdb_db = IMDBDatabase::new(db.deref());
            match imdb_db.update_metadata(&cached_item).await {
                Ok(_) => (),
                Err(e) => return Err(ErrorInternalServerError(e)),
            };
        }

        create_modal_body_imdb(&cached_item)
    };

    Ok(HttpResponse::Ok().message_body(body).unwrap())
}

// IMDB Functions
async fn get_cached_item_imdb(id: &str, db: Data<DBConnection>) -> anyhow::Result<IMDBItem> {
    let imdb_db = IMDBDatabase::new(db.deref());
    let items = imdb_db.fetch_item_by_id(id).await?;
    if items.is_empty() {
        return Err(format_err!("Failed to fetch item"));
    }
    let item = items.into_iter().next().unwrap();

    Ok(item)
}

fn create_modal_body_imdb(item: &IMDBItem) -> String {
    let title = &item.title;
    let mut subheading = format!("{} | {}", item.year, &item.rating);

    if let Some(t) = item.runtime {
        let minutes = format!(" | {}m", t.div(60));
        subheading.push_str(&minutes);
    };

    let default_plot = "".to_string();
    let plot = item.plot.as_ref().unwrap_or(&default_plot);

    let heading = format!(
        "<div>\
    <h2>{}</h2>\
    <p><small>{}</small></p>\
    <p>{}</p>\
    </div>",
        title, subheading, plot
    );

    let watchlist_button = super::download::create_watchlist_button(&item.id, item.watchlist);
    let accordion = create_accordion_imdb(item);

    let html = format!("<div id=\"download-select\">{}{}<div id=\"modal_accordion\" class=\"accordion\">{}</div></div>", heading, watchlist_button, accordion);

    html
}

fn create_accordion_imdb(item: &IMDBItem) -> String {
    let video_url = match &item.video_url {
        Some(t) => t.to_string(),
        None => "".to_string(),
    };

    let trailer_segment = format!("<div class=\"accordion-item\">\
        <h3 class=\"accordion-header\">\
            <button class=\"accordion-button\" type=\"button\" data-bs-toggle=\"collapse\" data-bs-target=\"#collapseTrailer\" aria-expanded=\"true\" aria-controls=\"collapseTrailer\">\
                Trailer\
            </button>\
        </h3>\
        <div id=\"collapseTrailer\" class=\"accordion-collapse collapse show\" data-bs-parent=\"#modal_accordion\">\
            <div class=\"accordion-body\" style=\"width=100%\" >\
                <iframe id=\"player\" style=\"width=100%; height: auto;\" type=\"text/html\" src=\"https://www.youtube.com/embed/{}\" frameborder=\"0\"></iframe>
            </div>\
        </div>\
    </div>", video_url);

    let _type = match item._type {
        ItemType::Movie => "movie",
        ItemType::TvShow => "tv",
    };

    let id = match item.id.strip_prefix("tt") {
        Some(t) => t,
        None => &item.id,
    };

    let title = format!("{} ({})", &item.title, &item.year);
    let title_encoded = urlencoding::encode(&title);

    let download_segment = format!("<div class=\"accordion-item\">\
        <h3 class=\"accordion-header\">\
            <button class=\"accordion-button collapsed\" type=\"button\" data-bs-toggle=\"collapse\"  data-bs-target=\"#collapseDownload\" aria-expanded=\"false\" aria-controls=\"collapseDownload\">\
                Downloads\
            </button>\
        </h3>\
        <div id=\"collapseDownload\" class=\"accordion-collapse collapse\" data-bs-parent=\"#modal_accordion\">\
            <div class=\"accordion-body\">\
                <div id=\"load-spinner-accordion\" class=\"htmx-indicator spinner-border\" hx-get=\"/find_download?imdb_id={}&title={}&type={}\" hx-swap=\"outerHTML\" hx-trigger=\"load\" hx-indicator=\"#load-spinner-accordion\"></div>
            </div>\
        </div>\
    </div>", id, title_encoded, _type);

    format!("{}{}", trailer_segment, download_segment)
}
