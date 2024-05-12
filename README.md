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
![Docker](https://img.shields.io/badge/docker-%25.svg?style=flat&logo=docker&logoColor=white&color=2496ED)

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

## TODO

- Fix episodes on TheRARBG not being shown
    - Ensure additional pages are being checked
- Add check to prevent same media being downloaded before plex tracks it.
    - Ensure applied to Watchlist and requested media