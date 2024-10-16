use std::ops::{Deref, Not};
use std::sync::Arc;

use actix_web::{Error, get, HttpResponse, post, web};
use actix_web::error::ErrorInternalServerError;
use actix_web::web::{Data, Json, Query};
use rayon::prelude::*;
use serde::Deserialize;

use crate::api::imdb::{IMDB, IMDBEpisode, ItemType};
use crate::api::plex::Plex;
use crate::api::torrent::{MediaQuality, Torrenter, TorrentItem};
use crate::AppConfig;
use crate::db::DBConnection;
use crate::db::downloads::DownloadDatabase;
use crate::db::imdb::IMDBDatabase;

#[derive(Deserialize)]
pub struct DownloadQueryParams {
    imdb_id: String,
    title: String,
    #[serde(rename = "type")]
    _type: String,
    ignore_already_exists: Option<bool>,
}

#[derive(Deserialize, Debug)]
pub struct TorrentQuery {
    pub imdb_id: String,
    #[serde(default)]
    pub season: Option<i32>,
    #[serde(default)]
    pub episode: Option<i32>,
    pub quality: MediaQuality,
    pub magnet_uri: String,
}

#[get("/find_download")]
pub async fn find_download(
    params: Query<DownloadQueryParams>,
    plex: Data<Plex>,
    db: Data<DBConnection>,
    torrenter: Data<Torrenter>,
    app_config: Data<AppConfig>,
) -> Result<HttpResponse<String>, Error> {
    let concurrent_search = app_config.concurrent_torrent_search;
    let missing_tv_episodes = match params._type.as_str() {
        "tv" => {
            match find_missing_tv_shows(
                plex.clone().into_inner(),
                &params.imdb_id,
                &params.title,
            )
            .await
            {
                Ok(t) => t,
                Err(e) => return Err(ErrorInternalServerError(e)),
            }
        }
        _ => None,
    };

    let already_exists = missing_tv_episodes.is_none()
        && match plex.exists_in_library(&params.title, false).await {
            Ok(b) => b,
            Err(e) => return Err(ErrorInternalServerError(e)),
        };

    // TODO: Add check to prevent downloading active downloads
    let download_db = DownloadDatabase::new(db.deref());

    let imdb_id = match params.imdb_id.starts_with("tt") {
        true => params.imdb_id.to_owned(),
        false => format!("tt{}", params.imdb_id),
    };

    let (is_downloading, missing_tv_episodes) = match download_db
        .is_downloading(&imdb_id, missing_tv_episodes)
        .await
    {
        Ok(t) => t,
        Err(e) => return Err(ErrorInternalServerError(e)),
    };

    if already_exists && !params.ignore_already_exists.is_some_and(|x| x) {
        return Ok(HttpResponse::Ok()
            .message_body("<b>Content already exists</b>".to_string())
            .unwrap());
    }

    if is_downloading
        && (missing_tv_episodes.is_none()
            || missing_tv_episodes.as_ref().is_some_and(|x| x.is_empty()))
    {
        return Ok(HttpResponse::Ok()
            .message_body("<b>Content is already downloading</b>".to_string())
            .unwrap());
    }

    // Find Torrent on first platform that has a download
    let torrents = match torrenter
        .find_torrent(
            params.title.to_owned(),
            Some(params.imdb_id.to_owned()),
            missing_tv_episodes,
            concurrent_search,
        )
        .await
    {
        Ok(t) => t,
        Err(e) => {
            return Ok(HttpResponse::Ok()
                .message_body(format!("<b>{}</b>", e))
                .unwrap())
        }
    };

    let output = create_download_modal_options(torrents);

    Ok(HttpResponse::Ok().message_body(output).unwrap())
}
fn create_download_modal_options(items: Vec<TorrentItem>) -> String {
    let _type = match items.first() {
        Some(t) => t._type.clone(),
        None => ItemType::Movie,
    };

    let all_download_button_qualities = [MediaQuality::_1080p, MediaQuality::_720p];

    match _type {
        ItemType::Movie => {
            let select = items
                .par_iter()
                .map(create_download_movie_modal_button)
                .collect::<Vec<String>>()
                .join("");

            format!(
                "<div id=\"download_selection\" style=\"display: flex; flex-direction: column;\">\
    {}\
</div>",
                select
            )
        }
        ItemType::TvShow => {
            let mut output = String::new();
            output.push_str("<div>");

            generate_season_download_buttons(&items, &all_download_button_qualities, &mut output);

            let seasons =
                items.chunk_by(|a, b| a.season.as_ref().unwrap() == b.season.as_ref().unwrap());

            output.push_str("<div id=\"season_accordion\" class=\"accordion\">");
            for season in seasons {
                let season_number = season.iter().next().unwrap().season.unwrap();
                let accordion_item = format!("<div class=\"accordion-item\">\
        <h3 class=\"accordion-header\">\
            <button class=\"accordion-button collapsed\" type=\"button\" data-bs-toggle=\"collapse\" data-bs-target=\"#collapseSeason{}\" aria-expanded=\"false\" aria-controls=\"collapseSeason{}\">\
                Season {}\
            </button>\
        </h3>\
        <div id=\"collapseSeason{}\" class=\"accordion-collapse collapse\" data-bs-parent=\"#season_accordion\">", season_number, season_number, season_number, season_number);
                output.push_str(&accordion_item);
                output.push_str("<div style=\"display: flex; flex-direction: column;\">");

                generate_season_download_buttons(
                    season,
                    &all_download_button_qualities,
                    &mut output,
                );

                for item in season {
                    let btn_colour = button_colour_for_quality(&item.quality);

                    let imdb_id = match item.imdb_id.starts_with("tt") {
                        true => item.imdb_id.clone(),
                        false => format!("tt{}", item.imdb_id),
                    };

                    let episode = item.episode.as_ref().unwrap();
                    if *episode == -1 {
                        let button = format!("\
                <button class=\"download-button btn btn-{}\" hx-post=\"/start_download\" hx-vals='{{\"queries\":[{{\"imdb_id\": \"{}\", \"season\": {}, \"quality\": \"{}\", \"magnet_uri\": \"{}\"}}]}}' hx-ext='json-enc' hx-swap=\"outerHTML\" hx-disabled-elt=\"closest button\" hx-confirm=\"Start download?\">\
                    Entire Season {} - {} [{}]\
                </button>", btn_colour, imdb_id, item.season.as_ref().unwrap(), item.quality, urlencoding::encode(&item.magnet_uri), item.season.as_ref().unwrap(), item.quality, item.source);
                        output.push_str(&button);
                    } else {
                        let button = format!("\
                <button class=\"download-button btn btn-{}\" hx-post=\"/start_download\" hx-vals='{{\"queries\":[{{\"imdb_id\": \"{}\", \"season\": {}, \"episode\": {}, \"quality\": \"{}\", \"magnet_uri\": \"{}\"}}]}}' hx-ext='json-enc' hx-swap=\"outerHTML\" hx-disabled-elt=\"closest button\" hx-confirm=\"Start download?\">\
                    Season: {} Episode: {} - {} [{}]\
                </button>", btn_colour, imdb_id, item.season.as_ref().unwrap(), episode, item.quality, urlencoding::encode(&item.magnet_uri), item.season.as_ref().unwrap(), episode, item.quality, item.source);

                        output.push_str(&button);
                    }
                }

                output.push_str("</div>");
                output.push_str(
                    "</div>\
        </div>\
    </div>",
                );
            }

            output.push_str("</div>");

            output
        }
    }
}

fn generate_season_download_buttons(
    items: &[TorrentItem],
    qualities: &[MediaQuality],
    output: &mut String,
) {
    for quality in qualities {
        let all_matching_quality = all_torrents_for_quality(items, *quality);

        if all_matching_quality.is_empty().not() {
            let vals = all_matching_quality.par_iter().filter(|x| x.episode.as_ref().unwrap().ge(&0) ).map(|v| {
                let imdb_id = match v.imdb_id.starts_with("tt") {
                    true => v.imdb_id.clone(),
                    false => format!("tt{}", v.imdb_id),
                };
                format!("{{\"imdb_id\": \"{}\", \"season\": {}, \"episode\": {}, \"quality\": \"{}\", \"magnet_uri\": \"{}\"}}", imdb_id, v.season.unwrap(), v.episode.unwrap(), v.quality, urlencoding::encode(&v.magnet_uri))
            }).collect::<Vec<String>>().join(",");

            let download_all_button = format!("\
                <button class=\"download-button-all btn btn-success btn-lg\" hx-post=\"/start_download\" hx-vals='{{\"queries\":[{}]}}' hx-ext='json-enc'  hx-disabled-elt=\"this\" hx-confirm=\"Start download?\">\
                    Download All ({})
                </button>", vals, quality);
            output.push_str(download_all_button.as_str());
        }
    }
}

fn all_torrents_for_quality(items: &[TorrentItem], quality: MediaQuality) -> Vec<&TorrentItem> {
    items.par_iter().filter(|i| i.quality == quality).collect()
}

fn create_download_movie_modal_button(item: &TorrentItem) -> String {
    let imdb_id = match item.imdb_id.starts_with("tt") {
        true => item.imdb_id.clone(),
        false => format!("tt{}", item.imdb_id),
    };
    let value = format!(
        "hx-vals='{{\"queries\":[{{\"imdb_id\": \"{}\", \"quality\": \"{}\", \"magnet_uri\": \"{}\"}}]}}'",
        imdb_id, item.quality, urlencoding::encode(&item.magnet_uri)
    );

    let btn_colour = button_colour_for_quality(&item.quality);

    format!("<button class=\"download-button btn btn-{}\" hx-post=\"/start_download\" hx-ext='json-enc' hx-confirm=\"Start download?\" hx-swap=\"outerHTML\" hx-target=\"#download_selection\"{}>{} [{}]</button>", btn_colour, value, item.quality, item.source)
}

fn button_colour_for_quality(quality: &MediaQuality) -> &'static str {
    match quality {
        MediaQuality::Unknown => "danger",
        MediaQuality::Cam => "warning",
        MediaQuality::Telesync => "warning",
        MediaQuality::_720p => "info",
        MediaQuality::_1080p => "primary",
        MediaQuality::BetterThan1080p => "primary",
        MediaQuality::_480p => "warning",
        MediaQuality::_2160p => "dark",
        MediaQuality::_4320p => "secondary",
    }
}

pub async fn find_missing_tv_shows(
    plex: Arc<Plex>,
    imdb_id: &str,
    title: &str,
) -> anyhow::Result<Option<Vec<IMDBEpisode>>> {
    let mut all_episodes = match IMDB::search_tv_episodes(imdb_id, None, 0).await {
        Ok(t) => t,
        Err(e) => return Err(e),
    };

    let existing_episodes = match plex.tvshow_exists_in_library(title).await {
        Ok(t) => t,
        Err(e) => return Err(e),
    };
    for existing_episode in existing_episodes {
        if let Some((i, _)) = all_episodes.par_iter().enumerate().find_any(|(_, e)| {
            e.season == existing_episode.season && e.episode == existing_episode.episode
        }) {
            all_episodes.swap_remove(i);
        };
    }

    if all_episodes.is_empty() {
        Ok(None)
    } else {
        Ok(Some(all_episodes))
    }
}

#[get("/start_download")]
pub async fn start_download(
    params: Query<TorrentQuery>,
    torrenter: Data<Torrenter>,
) -> Result<HttpResponse, Error> {
    for magnet in params.magnet_uri.split(',') {
        let torrent_item = TorrentItem::new(
            params.imdb_id.clone(),
            "".to_string(),
            magnet.to_string(),
            params.quality,
            match params.season {
                Some(_) => ItemType::TvShow,
                None => ItemType::Movie,
            },
            params.season,
            params.episode,
            None,
            "unknown".to_string()
        );

        match torrenter.start_download(torrent_item).await {
            Ok(_) => (),
            Err(e) => return Err(ErrorInternalServerError(e)),
        };
    }

    Ok(HttpResponse::Ok().body("<b>Download Started!<b>"))
}

#[derive(Deserialize, Debug)]
// #[serde(transparent)]
struct TorrentQueries {
    queries: Vec<TorrentQuery>,
}

#[post("/start_download")]
pub async fn start_download_post(
    params: Json<TorrentQueries>,
    torrenter: Data<Torrenter>,
    db: Data<DBConnection>,
) -> Result<HttpResponse, Error> {
    let mut params = params;
    for data in params.queries.as_slice() {
        let torrent_item = TorrentItem::new(
            data.imdb_id.clone(),
            "".to_string(),
            urlencoding::decode(&data.magnet_uri).unwrap().to_string(),
            MediaQuality::Unknown,
            ItemType::Movie,
            None,
            None,
            None,
            "unknown".to_string()
        );

        match torrenter.start_download(torrent_item).await {
            Ok(_) => (),
            Err(e) => return Err(ErrorInternalServerError(e)),
        };
    }

    params.queries.par_iter_mut().for_each(|torrent| {
        let uri = torrent.magnet_uri.clone();
        let magnet = urlencoding::decode(&uri).unwrap();
        torrent.magnet_uri.clear();
        torrent.magnet_uri.push_str(&magnet);
    });

    match DownloadDatabase::new(&db)
        .insert_many(params.queries.as_slice())
        .await
    {
        Ok(_) => (),
        Err(e) => return Err(ErrorInternalServerError(e)),
    }

    Ok(HttpResponse::Ok().body("<b>Download Started!<b>"))
}

#[derive(Deserialize)]
struct UpdateWatchlistQuery {
    imdb_id: String,
    state: bool,
}

#[get("/update_watchlist")]
pub async fn update_watchlist(
    query: Query<UpdateWatchlistQuery>,
    db: web::Data<DBConnection>,
) -> Result<HttpResponse<String>, Error> {
    let button = {
        let imdb_db = IMDBDatabase::new(db.deref());

        match imdb_db
            .update_watchlist_item(&query.imdb_id, query.state)
            .await
        {
            Ok(_) => (),
            Err(e) => return Err(ErrorInternalServerError(e)),
        };

        create_watchlist_button(&query.imdb_id, query.state)
    };

    Ok(HttpResponse::Ok().message_body(button).unwrap())
}

pub fn create_watchlist_button(imdb_id: &str, state: bool) -> String {
    let mut button = format!("<div id=\"watchlist-button\"><button type=\"button\" class=\"btn btn-outline-secondary\" hx-target=\"#watchlist-button\" hx-get=\"/update_watchlist?imdb_id={}&state={}\">", imdb_id, !state);
    if state {
        button.push_str("Remove from watchlist");
    } else {
        button.push_str("Add to watchlist");
    }
    button.push_str("</button></div>");

    button
}
