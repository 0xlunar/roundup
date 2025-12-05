use std::fmt::{Display, Formatter};

pub trait TorrentScraper {
    type TorrentType;
    async fn search(query: Search) -> Result<Option<Vec<Self::TorrentType>>, ScraperError>;
}

pub enum Search<'a> {
    Query(&'a str),
    IMDb(IMDbId<'a>),
}

pub enum ScraperError {
    Anyhow(anyhow::Error),
}

#[derive(Debug, Clone, PartialEq)]
pub struct IMDbId<'a>(&'a str);
impl<'a> IMDbId<'a> {
    pub fn new(id: &'a str) -> Result<Self, String> {
        if id.len() < 9 {
            return Err(format!(
                "{id} is too short for an IMDb Id, must start with tt followed by at least 7 numbers (eg, tt0121955)"
            ));
        }

        let mut chars = id.chars().enumerate();
        while let Some((index, char)) = chars.next() {
            if ((index == 0 || index == 1) && char == 't') || (char.is_ascii_digit() && index > 1) {
                continue;
            } else {
                return Err(format!(
                    "{id} is not an IMDb Id, must start with tt followed by at least 7 numbers (eg, tt0121955)"
                ));
            }
        }
        Ok(Self(id))
    }

    pub fn as_str(&self) -> &str {
        self.0
    }
}
impl<'a> Display for IMDbId<'a> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

mod test {
    use crate::scrapers::IMDbId;

    #[test]
    fn valid_ids() {
        assert_eq!(
            IMDbId::new("tt0121955"),
            Ok(IMDbId("tt0121955")),
            "Failed to parse valid imdb id"
        );

        assert_eq!(
            IMDbId::new("tt0436992"),
            Ok(IMDbId("tt0436992")),
            "Failed to parse valid imdb id"
        );

        assert_eq!(
            IMDbId::new("tt4574334"),
            Ok(IMDbId("tt4574334")),
            "Failed to parse valid imdb id"
        );
    }

    #[test]
    fn errors_if_starts_with_tt_but_not_all_ascii_numeric_afterwards() {
        assert_eq!(
            IMDbId::new("tt128l173"),
            Err(
                "tt128l173 is not an IMDb Id, must start with tt followed by at least 7 numbers (eg, tt0121955)"
                    .to_string()
            ),
            "Got valid id for invalid id"
        );

        assert_eq!(
            IMDbId::new("ttabv12345"),
            Err(
                "ttabv12345 is not an IMDb Id, must start with tt followed by at least 7 numbers (eg, tt0121955)"
                    .to_string()
            ),
            "Got valid id for invalid id"
        );

        assert_eq!(
            IMDbId::new("tt8333838i"),
            Err(
                "tt8333838i is not an IMDb Id, must start with tt followed by at least 7 numbers (eg, tt0121955)"
                    .to_string()
            ),
            "Got valid id for invalid id"
        );
    }

    #[test]
    fn errors_if_starts_with_numbers() {
        assert_eq!(
            IMDbId::new("012195512"),
            Err(
                "012195512 is not an IMDb Id, must start with tt followed by at least 7 numbers (eg, tt0121955)"
                    .to_string()
            ),
            "Got valid id for invalid id"
        );

        assert_eq!(
            IMDbId::new("012195573"),
            Err(
                "012195573 is not an IMDb Id, must start with tt followed by at least 7 numbers (eg, tt0121955)"
                    .to_string()
            ),
            "Got valid id for invalid id"
        );

        assert_eq!(
            IMDbId::new("012195551"),
            Err(
                "012195551 is not an IMDb Id, must start with tt followed by at least 7 numbers (eg, tt0121955)"
                    .to_string()
            ),
            "Got valid id for invalid id"
        );
    }

    #[test]
    fn errors_if_starts_with_non_tt_character() {
        assert_eq!(
            IMDbId::new("aa2195512"),
            Err(
                "aa2195512 is not an IMDb Id, must start with tt followed by at least 7 numbers (eg, tt0121955)"
                    .to_string()
            ),
            "Got valid id for invalid id"
        );

        assert_eq!(
            IMDbId::new("cx2195573"),
            Err(
                "cx2195573 is not an IMDb Id, must start with tt followed by at least 7 numbers (eg, tt0121955)"
                    .to_string()
            ),
            "Got valid id for invalid id"
        );

        assert_eq!(
            IMDbId::new("du2195551"),
            Err(
                "du2195551 is not an IMDb Id, must start with tt followed by at least 7 numbers (eg, tt0121955)"
                    .to_string()
            ),
            "Got valid id for invalid id"
        );
    }

    #[test]
    fn errors_if_less_than_9_characters() {
        assert_eq!(
            IMDbId::new("tt01219"),
            Err(
                "tt01219 is too short for an IMDb Id, must start with tt followed by at least 7 numbers (eg, tt0121955)"
                    .to_string()
            ),
            "Got valid id for invalid id"
        );

        assert_eq!(
            IMDbId::new("b1012195"),
            Err(
                "b1012195 is too short for an IMDb Id, must start with tt followed by at least 7 numbers (eg, tt0121955)"
                    .to_string()
            ),
            "Got valid id for invalid id"
        );

        assert_eq!(
            IMDbId::new("aa012391"),
            Err(
                "aa012391 is too short for an IMDb Id, must start with tt followed by at least 7 numbers (eg, tt0121955)"
                    .to_string()
            ),
            "Got valid id for invalid id"
        );
    }
}
