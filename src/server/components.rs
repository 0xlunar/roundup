use crate::database::torrent::TorrentDBItemWithIMDB;
use crate::scrapers::imdb::{IMDbDetailedItem, IMDbItem};
use crate::scrapers::{IMDbMediaType, Torrent, TorrentMediaType};
use itertools::Itertools;
use maud::{html, Markup};
use std::cmp::Ordering;
use std::collections::hash_map::Entry;
use std::collections::HashMap;

#[derive(Debug)]
pub enum ToastVariant {
    Info(String),
    Success(String),
    Warning(String),
    Error(String),
}

pub fn toast(variant: ToastVariant) -> Markup {
    html! {
        @match variant {
            ToastVariant::Info(msg) => {
                div class="alert alert-info" onclick="dismiss_toast(this)" {
                    svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" class="h-6 w-6 shrink-0 stroke-current" {
                        path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M13 16h-1v-4h-1m1-4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z" {}
                    }
                    span {
                        (msg)
                    }
                }
            }
            ToastVariant::Success(msg) => {
                div class="alert alert-success" onclick="dismiss_toast(this)" {
                    svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" class="h-6 w-6 shrink-0 stroke-current" {
                        path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M9 12l2 2 4-4m6 2a9 9 0 11-18 0 9 9 0 0118 0z" {}
                    }
                    span {
                        (msg)
                    }
                }
            }
            ToastVariant::Warning(msg) => {
                div class="alert alert-warning" onclick="dismiss_toast(this)" {
                    svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" class="h-6 w-6 shrink-0 stroke-current" {
                        path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-3L13.732 4c-.77-1.333-2.694-1.333-3.464 0L3.34 16c-.77 1.333.192 3 1.732 3z" {}
                    }
                    span {
                        (msg)
                    }
                }
            }
            ToastVariant::Error(msg) => {
                div class="alert alert-error" onclick="dismiss_toast(this)" {
                    svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" class="h-6 w-6 shrink-0 stroke-current" {
                        path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M10 14l2-2m0 0l2-2m-2 2l-2-2m2 2l2 2m7-2a9 9 0 11-18 0 9 9 0 0118 0z" {}
                    }
                    span {
                        (msg)
                    }
                }
            }
        }
    }
}

pub fn skeleton_card() -> Markup {
    html! {
        div class="flex w-48 flex-col gap-4" {
            div class="skeleton h-64 w-full" {}
        }
    }
}

pub fn item_card(item: &IMDbItem) -> Markup {
    html! {
        div class="flex flex-col w-48 gap-4" hx-get=(format!("/metadata?id={}", item.id)) hx-target="#metadata" hx-indicator="#metadata-skeleton" hx-swap="outerHTML"  {
            div class="card bg-base-300 h-64 cursor-pointer" {
                figure class="rounded-lg"{
                    @match &item.image_url {
                        Some(image) => {
                            img src=(image) alt=(item.title) {}
                        },
                        None => {
                            img alt=(format!("{} ({})", item.title, item.year)) {}
                        }
                    }
                }
                div class="group absolute h-full w-full" {
                    div class="invisible group-hover:visible bg-base-300/70 h-full w-full rounded-lg" {
                        div class="card-body" {
                            h2 class="card-title" {
                                (item.title)
                            }
                            p {
                                (item.year)
                            }
                        }
                    }
                }
            }
        }
    }
}

pub fn download_item_card(item: &TorrentDBItemWithIMDB) -> Markup {
    html! {
        div class="flex flex-col w-48 gap-4" {
            div class="card bg-base-300 h-64 cursor-pointer" {
                figure class="rounded-lg" {
                    @match &item.imdb_item.image_url {
                        Some(image) => {
                            img src=(image) alt=(item.imdb_item.title) {}
                        },
                        None => {
                            img alt=(format!("{} ({})", item.imdb_item.title, item.imdb_item.year)) {}
                        }
                    }
                }
                div class="group absolute h-full w-full" {
                    div class="bg-base-300/70 h-full w-full rounded-lg" {
                        div class="card-body h-full" {
                            h2 class="card-title" {
                                (item.imdb_item.title)
                            }
                            div class="flex flex-col h-full" {
                                div class="grow"{
                                    p {
                                        (item.imdb_item.year)
                                    }
                                    @match item.imdb_item._type {
                                        IMDbMediaType::TvShow => {
                                            @if let (Some(season), Some(episode)) = (item.torrent_item.season, item.torrent_item.episode) {
                                                p {
                                                    (format!("Season: {season} - Episode: {episode}"))
                                                }
                                            } @else if let Some(season) = item.torrent_item.season {
                                                p {
                                                    (format!("Season: {season}"))
                                                }
                                            } @else if let Some(episode) = item.torrent_item.episode {
                                                p {
                                                    (format!("Episode: {episode}"))
                                                }
                                            }
                                        }
                                        _ => {}
                                    }
                                }
                                div class="justify-end" {
                                    p {
                                        (item.torrent_item.state)
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

pub fn item_cards(items: &[IMDbItem]) -> Markup {
    html! {
        @for item in items {
            (item_card(item))
        }
    }
}

pub fn download_item_cards(items: &[TorrentDBItemWithIMDB]) -> Markup {
    html! {
        @for item in items {
            (download_item_card(item))
        }
    }
}

pub fn metadata_modal(item: IMDbDetailedItem) -> Markup {
    let image_url = item.item.image_url.unwrap_or("".to_string());

    html! {
        div class="modal-box lg:min-w-3xl" {
            div class="flex flex-row flex-wrap gap-4" {
                figure class="hidden lg:block grow" {
                    img class="rounded-lg w-48" src=(image_url) alt=(item.item.title) {}
                }
                div class="flex flex-col gap-4" {
                    div class="grow" {
                        a {
                            h2 class="card-title" {
                                (item.item.title)
                            }
                            p {
                                (item.item.year)
                            }
                        }
                        p class="text-wrap lg:max-w-lg" {
                            (item.plot)
                        }
                    }
                    div class="items-end" {
                        fieldset class="fieldset" {
                            button class="btn btn-soft btn-accent" hx-post="/api/watchlist" {
                                "Add to watchlist"
                            }
                            div id="download-actions-skeleton" class="flex skeleton h-16 w-full items-center justify-center" hx-swap="outerHTML" hx-get=(format!("/api/fetch-downloads?imdb_id={}", item.item.id)) {
                                span class="skeleton skeleton-text text-xl" {
                                    "Fetching Downloads..."
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

pub fn movie_download_form(items: Vec<Torrent>) -> Markup {
    let items = items
        .into_iter()
        .sorted_by(|a, b| b.media_quality.cmp(&a.media_quality));

    let mut hashmap = HashMap::<String, Vec<Torrent>>::new();
    for item in items {
        if item.media_type != TorrentMediaType::Movie {
            continue;
        }
        match hashmap.entry(item.source.clone()) {
            Entry::Occupied(o) => {
                o.into_mut().push(item);
            }
            Entry::Vacant(v) => {
                v.insert(vec![item]);
            }
        }
    }

    html! {
        div id="download-actions" {
            form class="flex flex-row gap-4" name="movie-download-form" hx-post="/api/download" {
                fieldset class="fieldset flex flex-row flex-grow gap-4" {
                    legend class="fieldset-legend" {
                        "Download"
                    }
                    select name="quality" class="select flex flex-grow" {
                        option disabled selected {
                            "Choose quality"
                        }
                        @for (source, items) in hashmap.into_iter() {
                            legend class="fieldset-legend" {
                                (source)
                            }
                            @for item in items {
                                @match item.torrent.to_hash() {
                                    Some(hash) => option value=(hash) { (item.media_quality) },
                                    None => continue;,
                                }
                            }
                        }
                    }
                    button type="submit" value="Submit" class="btn btn-soft btn-accent" {
                        "Download"
                    }
                }
            }
        }
    }
}

pub fn tv_show_download_form(items: Vec<Torrent>) -> Markup {
    let mut season_hashmap = HashMap::<i64, Vec<Torrent>>::new();
    for item in items {
        if item.media_type == TorrentMediaType::Movie {
            continue;
        }

        let season = match item.media_type {
            TorrentMediaType::Movie => unreachable!("Item already filtered"),
            TorrentMediaType::TvShowEpisode { season, episode } => season,
            TorrentMediaType::TvShowSeason { season } => season,
            TorrentMediaType::TvShowSeasonPack {
                season_first,
                season_last,
            } => season_first,
        };

        match season_hashmap.entry(season) {
            Entry::Occupied(o) => {
                o.into_mut().push(item);
            }
            Entry::Vacant(v) => {
                v.insert(vec![item]);
            }
        }
    }

    for (season, vec) in season_hashmap.iter_mut() {
        vec.sort_by(|a, b| match b.media_type {
            TorrentMediaType::TvShowEpisode { season: _, episode: b_episode } => {
                if matches!(a.media_type, TorrentMediaType::TvShowEpisode { season: _, episode: a_episode } if b_episode > a_episode) {
                    Ordering::Greater
                } else {
                    Ordering::Less
                }
            }
            _ => Ordering::Greater,
        })
    }

    html! {
        div id="download-actions" {
            @for (season, items) in season_hashmap {
                div class="collapse collapse-plus bg-base-100 border border-base-300" {
                    input type="checkbox" name="season-accordion" {}
                    div class="collapse-title font-semibold" {
                        (format!("Season {}", season))
                    }
                    div class="collapse-content" {
                        button class="btn btn-soft btn-accent" {
                            "Download season"
                        }
                        @for item in items {
                            form class="flex flex-row gap-4" name="download-form" hx-post="/api/download" {
                                fieldset class="fieldset flex flex-row flex-grow gap-4" {
                                    legened class="fieldset-legend" {
                                        @match item.media_type {
                                            TorrentMediaType::Movie => {
                                                continue;
                                            }
                                            TorrentMediaType::TvShowEpisode{ season: _, episode } => {
                                                (format!("Episode {episode}"))
                                            }
                                            TorrentMediaType::TvShowSeason{ season } => {
                                                (format!("Entire Season {season}"))
                                            }
                                            TorrentMediaType::TvShowSeasonPack{ season_first, season_last } => {
                                                (format!("Seasons {season_first} - {season_last}"))
                                            }
                                        }
                                    }
                                }
                            }
                        }

                    }
                }
            }
        }
    };

    todo!("Finish TV Episode selections and season downloading")
}
