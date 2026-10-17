use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::Value;

const MAX_VOTES: i64 = 1_000_000_000_000;

/// One rating on its source's native scale, from 0 to `scaleMax`.
#[derive(Debug, Clone, PartialEq, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub(super) struct NormalizedRating {
    source_id: &'static str,
    value: f64,
    /// The source's 0 to 100 score, when it reports one.
    score: Option<f64>,
    votes: Option<i64>,
    scale_max: f64,
}

pub(super) fn normalize_rating_array(value: &Value) -> Vec<NormalizedRating> {
    let mut ratings = Vec::new();
    for rating in value.as_array().into_iter().flatten() {
        let Some(source) = rating
            .get("sourceId")
            .and_then(Value::as_str)
            .and_then(canonical_source)
        else {
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
        ratings.push(NormalizedRating {
            source_id: source,
            value,
            score,
            votes,
            scale_max: scale,
        });
    }
    deduplicate_ratings(ratings)
}

/// The rating cache stores a normalized list as its JSON.
pub(super) fn rating_cache_value(ratings: &[NormalizedRating]) -> Value {
    serde_json::to_value(ratings).unwrap_or_else(|_| Value::Array(Vec::new()))
}

fn deduplicate_ratings(ratings: Vec<NormalizedRating>) -> Vec<NormalizedRating> {
    let mut by_source = BTreeMap::new();
    for rating in ratings {
        by_source.insert(rating.source_id, rating);
    }
    by_source.into_values().collect()
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

/// A rating source the app can show, with how its value is written.
#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub(super) struct RatingSourceDefinition {
    id: &'static str,
    label: &'static str,
    short_label: &'static str,
    scale_max: f64,
    format: RatingFormat,
    known: bool,
}

#[derive(Clone, Copy, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
enum RatingFormat {
    Percent,
    Decimal,
    Integer,
    Stars,
}

pub(super) fn known_source_definitions() -> Vec<RatingSourceDefinition> {
    use RatingFormat::{Decimal, Integer, Percent, Stars};
    vec![
        source_definition("mdblist_score", "MDBList Score", "MDB", 100.0, Percent),
        source_definition(
            "mdblist_score_average",
            "MDBList Score Average",
            "AVG",
            100.0,
            Percent,
        ),
        source_definition("imdb", "IMDb", "IMDb", 10.0, Decimal),
        source_definition("trakt", "Trakt", "Trakt", 100.0, Percent),
        source_definition("tmdb", "TMDB", "TMDB", 10.0, Decimal),
        source_definition("letterboxd", "Letterboxd", "LB", 5.0, Stars),
        source_definition("tomatoes", "Rotten Tomatoes Critics", "RT", 100.0, Percent),
        source_definition(
            "popcorn",
            "Rotten Tomatoes Audience",
            "RT A",
            100.0,
            Percent,
        ),
        source_definition("metacritic", "Metacritic Critics", "MC", 100.0, Integer),
        source_definition("metacriticuser", "Metacritic Users", "MC U", 10.0, Decimal),
        source_definition("rogerebert", "Roger Ebert", "Ebert", 4.0, Stars),
        source_definition("myanimelist", "MyAnimeList", "MAL", 10.0, Decimal),
    ]
}

fn source_definition(
    id: &'static str,
    label: &'static str,
    short_label: &'static str,
    scale_max: f64,
    format: RatingFormat,
) -> RatingSourceDefinition {
    RatingSourceDefinition {
        id,
        label,
        short_label,
        scale_max,
        format,
        known: true,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn normalization_uses_native_scales_and_drops_unknown_sources() {
        let ratings = normalize_rating_array(&json!([
            { "sourceId": "mdblist_score", "value": 84, "score": 84 },
            { "sourceId": "mdblist_score_average", "value": 81, "score": 81 },
            { "sourceId": "imdb", "value": 8.1, "score": 81, "votes": 10 },
            { "sourceId": "letterboxd", "value": 8, "score": 80 },
            { "sourceId": "tomatoes", "value": 96, "score": 97 },
            { "sourceId": "popcorn", "value": 91, "score": 91 },
            { "sourceId": "future_meter", "value": 7.25, "score": 73 }
        ]));
        assert_eq!(
            rating_cache_value(&ratings),
            json!([
                { "sourceId": "imdb", "value": 8.1, "score": 81.0, "votes": 10, "scaleMax": 10.0 },
                { "sourceId": "letterboxd", "value": 4.0, "score": 80.0, "votes": null, "scaleMax": 5.0 },
                { "sourceId": "mdblist_score", "value": 84.0, "score": 84.0, "votes": null, "scaleMax": 100.0 },
                { "sourceId": "mdblist_score_average", "value": 81.0, "score": 81.0, "votes": null, "scaleMax": 100.0 },
                { "sourceId": "popcorn", "value": 91.0, "score": 91.0, "votes": null, "scaleMax": 100.0 },
                { "sourceId": "tomatoes", "value": 96.0, "score": 97.0, "votes": null, "scaleMax": 100.0 }
            ])
        );
    }
}
