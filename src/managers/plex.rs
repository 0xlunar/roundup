use std::fmt::{Display, Formatter};
use wreq::Client;

pub struct PlexManager {
    client: Client,
    auth_token: String,
}

#[derive(Debug)]
pub enum PlexManagerError {
    MissingAuthToken(String),
    Error(anyhow::Error),
}

impl PlexManager {
    pub fn new(client: Client) -> Result<Self, PlexManagerError> {
        let auth_token = Self::find_local_auth_token()?;
        Ok(Self { client, auth_token })
    }

    pub async fn find_media(&self) -> () {
        todo!()
    }
    pub async fn delete_media(&self, id: &str) -> Result<(), PlexManagerError> {
        todo!()
    }

    async fn find_movie_in_library() -> () {
        todo!()
    }

    async fn find_tv_show_in_library() -> () {
        todo!()
    }

    #[cfg(target_os = "windows")]
    fn find_local_auth_token() -> Result<String, PlexManagerError> {
        use winreg::RegKey;
        use winreg::enums::HKEY_CURRENT_USER;

        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let plex_reg = match hkcu.open_subkey("Software\\Plex, Inc.\\Plex Media Server") {
            Ok(key) => key,
            Err(err) => return Err(PlexManagerError::MissingAuthToken(format!("{err}"))),
        };
        let token: String = match plex_reg.get_value("PlexOnlineToken") {
            Ok(token) => token,
            Err(err) => return Err(PlexManagerError::MissingAuthToken(format!("{err}"))),
        };

        Ok(token)
    }

    #[cfg(target_os = "linux")]
    fn find_local_auth_token() -> Result<String, PlexManagerError> {
        use std::fs;
        use std::path::PathBuf;
        use xmltree::Element;

        let path: PathBuf = "/var/lib/plexmediaserver/Library/Application Support/Plex Media Server/Preferences.xml".into();
        let xml_file = match fs::read_to_string(path) {
            Ok(file) => file,
            Err(err) => return Err(PlexManagerError::MissingAuthToken(format!("{err}"))),
        };
        let xml = match Element::parse(xml_file.as_bytes()) {
            Ok(element) => element,
            Err(err) => return Err(PlexManagerError::MissingAuthToken(format!("{err}"))),
        };
        let token = match xml.attributes.get("PlexOnlineToken") {
            Some(token) => token,
            None => {
                return Err(PlexManagerError::MissingAuthToken(
                    "token attribute does not exist".to_string(),
                ));
            }
        };

        Ok(token.to_string())
    }

    #[cfg(target_os = "macos")]
    fn find_local_auth_token() -> Result<String, PlexManagerError> {
        use plist;
        use std::path::PathBuf;

        #[derive(Deserialize)]
        struct MacOSPlexPlist {
            #[serde(rename = "PlexOnlineToken")]
            plex_online_token: String,
        }

        let path: PathBuf = "~/Library/Preferences/com.plexapp.plexmediaserver.plist".into();
        let plist_file: MacOSPlexPlist = match plist::from_file(path) {
            Ok(file) => file,
            Err(err) => return Err(PlexManagerError::MissingAuthToken(format!("{err}"))),
        };
        Ok(plist_file.plex_online_token)
    }
}

impl Display for PlexManagerError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            PlexManagerError::MissingAuthToken(err) => {
                f.write_fmt(format_args!("MissingAuthToken: {err}"))
            }
            PlexManagerError::Error(err) => f.write_fmt(format_args!("error: {err}")),
        }
    }
}

impl core::error::Error for PlexManagerError {}
