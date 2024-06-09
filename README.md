# Roundup

Easy to use Movie/TV Show torrent aggregator

![Actix](https://img.shields.io/badge/actix-%25.svg?style=flat&logo=actix&logoColor=white&color=000000)
![Rust](https://img.shields.io/badge/rust-%25.svg?style=flat&logo=rust&logoColor=white&color=000000)
![PostgresQL](https://img.shields.io/badge/postgresql-%25.svg?style=flat&logo=postgresql&logoColor=white&color=4169E1)
![qBittorrent](https://img.shields.io/badge/qbittorrent-%25.svg?style=flat&logo=qbittorrent&logoColor=white&color=2F67BA)
![Plex](https://img.shields.io/badge/plex-%25.svg?style=flat&logo=plex&logoColor=black&color=EBAF00)
![htmx](https://img.shields.io/badge/htmx-%25.svg?style=flat&logo=htmx&logoColor=white&color=3366CC)
![Bootstrap](https://img.shields.io/badge/bootstrap-%25.svg?style=flat&logo=bootstrap&logoColor=white&color=7952B3)
![IMDb](https://img.shields.io/badge/imdb-%25.svg?style=flat&logo=imdb&logoColor=black&color=F5C518)

## Requirements

- Postgresql/Docker
- Plex Media Server
- qBittorrent
- Windows (linux support at some point)

## Features

- List and search any TV Show/Movie available on IMDB
- Preview plot, trailer, rating and runtime before downloading
- Find available torrents for selected media
    - Supports YTS, EZTV and TheRARBG
- Watchlist
    - List of your favourite media
    - Automatically find and start torrents when they become available
        - Movies are auto removed
        - TV Shows stay indefinitely (currently)

## Installing and Running

1) Install and
   setup [Plex Media Server](https://www.plex.tv/media-server-downloads/?cat=computer&plat=windows#plex-media-server)
2) Deploy [Postgresql](https://www.postgresql.org/download/) database (almost every version should work) (local or
   3rd-party hosting works)
3) Install and setup [qBittorrent](https://www.qbittorrent.org/download) with WebUI enabled (adjust config.json with
   your webui settings)
4) Extract release files and place into a folder of your choice (eg, C:\roundup)
5) Configure the config.json file
6) Run roundup executable
7) visit http://127.0.0.1:80/ or https://127.0.0.1:443/ if TLS is setup. (or the ip for server you've deployed roundup
   on.)

## Build from source

1) [Install Rust](https://www.rust-lang.org/tools/install)
2) `cargo build --release` or `cargo run --release`
    1) build is located at `target/release/roundup.exe`

## Using PWA

If you wish to use PWA for your mobile devices, you must setup TLS support, PWA doesn't like to work on non-public
facing servers, so you may get insecure connection errors on your browser but it is fine. App Icons also won't work.
(if someone has a fix, please open a PR)

## Notice about TheMovieDB

Currently it is not fully setup, and should not be used in it's current state. By not supplying an API Key in the config
file it will default to IMDb instead.

## Trailers not working?

Simply by adding an API Key for Youtube they should start to appear in your searches.
Note: Some trailers may not show for various reasons.

## Trackers

Currently, you will need to supply your own trackers for YTS, the other sites include their own in their magnets.
You can set them in the config.json file or in qBittorrent settings.

## TODO

- Fix episodes on TheRARBG not being shown
    - Ensure additional pages are being checked
- Fix TheRARBG not parsing certain torrents
- Fix IMDB not parsing numbers and other data.
- Add Season pack support for EZTV
- Fix no video id for youtube videos
- Add TheRARBG YAPS API as an alternative to scraping. (Don't replace as yaps goes down more often than base site)

## Contribute

If you are looking to contribute to the project, please fork and make pull requests to be reviewed.
Please be descriptive on the problem you're looking to solve and the plan to solve it in your PR.

## Support the Project

[![ko-fi](https://ko-fi.com/img/githubbutton_sm.svg)](https://ko-fi.com/W7W0YYBB7)

**ETH**: 0xB37A8d6EA028cad32fBF71167B12C2827EcE9766
