use std::collections::BTreeMap;

use serde_json::{Value, json};

const MAX_VOTES: i64 = 1_000_000_000_000;

/// Positive response allowlist shared by Companion responses and cache repair.
/// It reads only catalog source IDs and bounded numeric fields; all other JSON
/// is intentionally forgotten.
pub(super) fn normalize_rating_array(value: &Value) -> Value {
    let mut ratings = Vec::new();
    for rating in value.as_array().into_iter().flatten() {
        let source = rating
            .get("sourceId")
            .and_then(Value::as_str)
            .and_then(canonical_source)
            .or_else(|| {
                rating
                    .get("source")
                    .and_then(Value::as_str)
                    .and_then(canonical_source)
            });
        let Some(source) = source else {
            continue;
        };
        let raw_value = number(rating.get("value").or_else(|| rating.get("rating")));
        let score = number(rating.get("score")).filter(|score| (0.0..=100.0).contains(score));
        let Some((value, scale)) = native_value(source, raw_value, score) else {
            continue;
        };
        let votes = rating.get("votes").and_then(|votes| {
            votes
                .as_i64()
                .or_else(|| votes.as_str()?.parse::<i64>().ok())
                .filter(|votes| (0..=MAX_VOTES).contains(votes))
        });
        ratings.push(normalized_rating(source, scale, value, score, votes));
    }
    deduplicate_ratings(ratings)
}

fn deduplicate_ratings(ratings: Vec<Value>) -> Value {
    let mut by_source = BTreeMap::<&'static str, Value>::new();
    for rating in ratings {
        if let Some(source) = rating
            .get("sourceId")
            .and_then(Value::as_str)
            .and_then(canonical_source)
        {
            by_source.insert(source, rating);
        }
    }
    Value::Array(by_source.into_values().collect())
}

fn normalized_rating(
    source: &'static str,
    scale: f64,
    value: f64,
    score: Option<f64>,
    votes: Option<i64>,
) -> Value {
    json!({
        // Keep Desktop v1's legacy keys, but make every string a catalog
        // constant rather than copying provider data.
        "source": source,
        "sourceId": source,
        "rawSource": source,
        "value": value,
        "score": score,
        "votes": votes,
        "scaleMax": scale,
    })
}

fn number(value: Option<&Value>) -> Option<f64> {
    let value = value?;
    let number = value.as_f64().or_else(|| {
        let text = value.as_str()?.trim();
        (text.len() <= 32)
            .then(|| text.parse::<f64>().ok())
            .flatten()
    })?;
    number.is_finite().then_some(number)
}

fn native_value(source: &str, raw: Option<f64>, score: Option<f64>) -> Option<(f64, f64)> {
    let maximum = scale_max(source)?;
    if let Some(raw) = raw
        && raw >= 0.0
        && raw <= maximum
    {
        return Some((raw, maximum));
    }
    score
        .filter(|score| (0.0..=100.0).contains(score))
        .map(|score| (score * maximum / 100.0, maximum))
}

fn canonical_source(source: &str) -> Option<&'static str> {
    if source.len() > 128 {
        return None;
    }
    let compact = source.trim().to_ascii_lowercase().replace([' ', '-'], "_");
    match compact.as_str() {
        "mal" | "myanimelist" => Some("myanimelist"),
        "audience" | "popcorn" | "tomatoesaudience" | "tomatoes_audience" | "rtaudience"
        | "rt_audience" => Some("popcorn"),
        "tomato" | "tomatometer" | "rtomatoes" | "rt_critic" | "tomatoes" => Some("tomatoes"),
        "score" | "mdblist" | "mdblist_score" => Some("mdblist_score"),
        "scoreaverage" | "score_average" | "mdblist_score_average" => Some("mdblist_score_average"),
        "imdb" => Some("imdb"),
        "trakt" => Some("trakt"),
        "tmdb" => Some("tmdb"),
        "letterboxd" => Some("letterboxd"),
        "metacritic" => Some("metacritic"),
        "metacriticuser" => Some("metacriticuser"),
        "rogerebert" => Some("rogerebert"),
        _ => None,
    }
}

fn scale_max(source: &str) -> Option<f64> {
    match source {
        "mdblist_score"
        | "mdblist_score_average"
        | "trakt"
        | "tomatoes"
        | "popcorn"
        | "metacritic" => Some(100.0),
        "imdb" | "tmdb" | "metacriticuser" | "myanimelist" => Some(10.0),
        "letterboxd" => Some(5.0),
        "rogerebert" => Some(4.0),
        _ => None,
    }
}

pub(super) fn normalized_source_updated_at(value: Option<&str>) -> Option<String> {
    let value = value?.trim();
    let bytes = value.as_bytes();
    if !(20..=35).contains(&bytes.len())
        || !bytes.iter().all(u8::is_ascii)
        || bytes.get(4) != Some(&b'-')
        || bytes.get(7) != Some(&b'-')
        || bytes.get(10) != Some(&b'T')
        || bytes.get(13) != Some(&b':')
        || bytes.get(16) != Some(&b':')
        || bytes.last() != Some(&b'Z')
        || !bytes[..19]
            .iter()
            .enumerate()
            .all(|(index, byte)| matches!(index, 4 | 7 | 10 | 13 | 16) || byte.is_ascii_digit())
        || !bytes[19..bytes.len() - 1]
            .iter()
            .all(|byte| *byte == b'.' || byte.is_ascii_digit())
    {
        return None;
    }
    Some(value.to_string())
}

pub(super) fn known_source_definitions() -> Vec<Value> {
    vec![
        source_definition("mdblist_score", "MDBList Score", "MDB", 100.0, "percent"),
        source_definition(
            "mdblist_score_average",
            "MDBList Score Average",
            "AVG",
            100.0,
            "percent",
        ),
        source_definition("imdb", "IMDb", "IMDb", 10.0, "decimal"),
        source_definition("trakt", "Trakt", "Trakt", 100.0, "percent"),
        source_definition("tmdb", "TMDB", "TMDB", 10.0, "decimal"),
        source_definition("letterboxd", "Letterboxd", "LB", 5.0, "stars"),
        source_definition(
            "tomatoes",
            "Rotten Tomatoes Critics",
            "RT",
            100.0,
            "percent",
        ),
        source_definition(
            "popcorn",
            "Rotten Tomatoes Audience",
            "RT A",
            100.0,
            "percent",
        ),
        source_definition("metacritic", "Metacritic Critics", "MC", 100.0, "integer"),
        source_definition(
            "metacriticuser",
            "Metacritic Users",
            "MC U",
            10.0,
            "decimal",
        ),
        source_definition("rogerebert", "Roger Ebert", "Ebert", 4.0, "stars"),
        source_definition("myanimelist", "MyAnimeList", "MAL", 10.0, "decimal"),
    ]
}

fn source_definition(id: &str, label: &str, short_label: &str, scale: f64, format: &str) -> Value {
    json!({
        "id": id,
        "label": label,
        "shortLabel": short_label,
        "scaleMax": scale,
        "format": format,
        "known": true,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};

    use super::*;

    #[test]
    fn canonical_sources_cover_current_official_fields_and_aliases() {
        assert_eq!(canonical_source("letterboxd"), Some("letterboxd"));
        assert_eq!(canonical_source("tomatoes"), Some("tomatoes"));
        assert_eq!(canonical_source("audience"), Some("popcorn"));
        assert_eq!(canonical_source("tomatoesaudience"), Some("popcorn"));
        assert_eq!(canonical_source("mal"), Some("myanimelist"));
        assert_eq!(
            canonical_source("score_average"),
            Some("mdblist_score_average")
        );
        let ids = known_source_definitions()
            .into_iter()
            .map(|definition| definition["id"].as_str().expect("id").to_string())
            .collect::<HashSet<_>>();
        for required in [
            "letterboxd",
            "tomatoes",
            "popcorn",
            "metacritic",
            "metacriticuser",
            "rogerebert",
            "myanimelist",
        ] {
            assert!(ids.contains(required), "missing {required}");
        }
    }

    #[test]
    fn normalization_uses_native_scales_and_drops_unknown_source_text() {
        let ratings = normalize_rating_array(&json!([
                { "source": "mdblist_score", "value": 84, "score": 84 },
                { "source": "score_average", "value": 81, "score": 81 },
                { "source": "imdb", "value": 8.1, "score": 81, "votes": 10 },
                { "source": "letterboxd", "value": 8, "score": 80 },
                { "source": "tomatoes", "value": 97, "score": 97 },
                { "source": "audience", "value": 91, "score": 91 },
                { "source": "future-meter!", "value": 7.25, "score": 73 }
        ]));
        let by_source = ratings
            .as_array()
            .expect("ratings")
            .iter()
            .map(|rating| (rating["sourceId"].as_str().expect("source"), rating))
            .collect::<HashMap<_, _>>();
        assert_eq!(by_source["letterboxd"]["value"], 4.0);
        assert_eq!(by_source["letterboxd"]["scaleMax"], 5.0);
        assert_eq!(by_source["popcorn"]["value"], 91.0);
        assert!(!by_source.contains_key("future_meter"));
        assert_eq!(by_source["imdb"]["rawSource"], "imdb");
        assert!(by_source.contains_key("mdblist_score"));
        assert!(by_source.contains_key("mdblist_score_average"));
    }
}
