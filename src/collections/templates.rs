use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{
    CollectionSource, MediaType, RefreshCadence, ResultLimit, ResultOrdering, TemplateReference,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TemplateCategory {
    Trending,
    Popular,
    StreamingServices,
    TopRated,
    InTheaters,
    Upcoming,
    OnAir,
    Editorial,
    Custom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TemplatePictogram {
    Award,
    Binary,
    Blocks,
    Bone,
    BookOpen,
    Briefcase,
    Bug,
    CalendarClock,
    CalendarDays,
    Circle,
    Compass,
    Crosshair,
    Drama,
    Film,
    Flame,
    Ghost,
    Heart,
    Landmark,
    Languages,
    Laugh,
    ListVideo,
    MonitorPlay,
    Mountain,
    Music,
    Orbit,
    Palette,
    PawPrint,
    Popcorn,
    Rocket,
    Search,
    SlidersHorizontal,
    Sparkles,
    Star,
    Swords,
    Telescope,
    TrendingUp,
    Trophy,
    Tv,
    UsersRound,
    WandSparkles,
    Zap,
}

impl TemplateCategory {
    pub const ORDER: [Self; 9] = [
        Self::Trending,
        Self::Popular,
        Self::StreamingServices,
        Self::TopRated,
        Self::InTheaters,
        Self::Upcoming,
        Self::OnAir,
        Self::Editorial,
        Self::Custom,
    ];
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionTemplate {
    #[serde(flatten)]
    pub provenance: TemplateReference,
    pub title: String,
    pub description: String,
    pub category: TemplateCategory,
    pub pictogram: TemplatePictogram,
    pub source: CollectionSource,
    pub media_type: MediaType,
    pub limit: ResultLimit,
    pub ordering: ResultOrdering,
    pub cadence: RefreshCadence,
}

// This independently authored manifest contains no copied Silo Server text,
// source, or artwork.
pub fn catalog() -> Vec<CollectionTemplate> {
    let mut templates = movie_discover_templates();
    let series = templates
        .iter()
        .take(23)
        .cloned()
        .map(series_counterpart)
        .collect::<Vec<_>>();
    templates.extend(franchise_templates());
    templates.extend(discover_variants());
    templates.extend(mdblist_selectors());
    templates.extend(series);
    debug_assert_eq!(templates.len(), 122);
    templates
}

type TemplateSeed = (&'static str, &'static str, TemplateCategory, Value);

fn movie_discover_templates() -> Vec<CollectionTemplate> {
    discovery_seeds()
        .into_iter()
        .chain(release_seeds())
        .chain(editorial_seeds())
        .map(|(id, title, category, parameters)| {
            discover_template(id, title, category, MediaType::Movie, &parameters)
        })
        .collect()
}

fn discovery_seeds() -> Vec<TemplateSeed> {
    vec![
        (
            "trending-day",
            "Trending today",
            TemplateCategory::Trending,
            json!({"feed":"trending","window":"day"}),
        ),
        (
            "trending-week",
            "Trending this week",
            TemplateCategory::Trending,
            json!({"feed":"trending","window":"week"}),
        ),
        (
            "popular",
            "Popular movies",
            TemplateCategory::Popular,
            json!({"sortBy":"popularity.desc"}),
        ),
        (
            "popular-new",
            "Popular new releases",
            TemplateCategory::Popular,
            json!({"sortBy":"popularity.desc","releaseWindow":"recent"}),
        ),
        (
            "netflix",
            "Popular on Netflix",
            TemplateCategory::StreamingServices,
            json!({"watchProvider":"netflix"}),
        ),
        (
            "disney-plus",
            "Popular on Disney+",
            TemplateCategory::StreamingServices,
            json!({"watchProvider":"disney-plus"}),
        ),
        (
            "prime-video",
            "Popular on Prime Video",
            TemplateCategory::StreamingServices,
            json!({"watchProvider":"prime-video"}),
        ),
        (
            "apple-tv",
            "Popular on Apple TV+",
            TemplateCategory::StreamingServices,
            json!({"watchProvider":"apple-tv-plus"}),
        ),
        (
            "max",
            "Popular on Max",
            TemplateCategory::StreamingServices,
            json!({"watchProvider":"max"}),
        ),
        (
            "top-rated",
            "Top rated movies",
            TemplateCategory::TopRated,
            json!({"sortBy":"vote_average.desc","voteCountGte":300}),
        ),
        (
            "critics",
            "Critically acclaimed",
            TemplateCategory::TopRated,
            json!({"sortBy":"vote_average.desc","voteCountGte":1000}),
        ),
    ]
}

fn release_seeds() -> Vec<TemplateSeed> {
    vec![
        (
            "in-theaters",
            "Now in theaters",
            TemplateCategory::InTheaters,
            json!({"releaseWindow":"theaters"}),
        ),
        (
            "opening-soon",
            "Opening soon",
            TemplateCategory::InTheaters,
            json!({"releaseWindow":"opening-soon"}),
        ),
        (
            "upcoming",
            "Upcoming movies",
            TemplateCategory::Upcoming,
            json!({"releaseWindow":"upcoming"}),
        ),
        (
            "anticipated",
            "Most anticipated",
            TemplateCategory::Upcoming,
            json!({"releaseWindow":"upcoming","sortBy":"popularity.desc"}),
        ),
    ]
}

fn editorial_seeds() -> Vec<TemplateSeed> {
    vec![
        (
            "action",
            "Action picks",
            TemplateCategory::Editorial,
            json!({"genre":28}),
        ),
        (
            "comedy",
            "Comedy picks",
            TemplateCategory::Editorial,
            json!({"genre":35}),
        ),
        (
            "drama",
            "Drama picks",
            TemplateCategory::Editorial,
            json!({"genre":18}),
        ),
        (
            "horror",
            "Horror picks",
            TemplateCategory::Editorial,
            json!({"genre":27}),
        ),
        (
            "science-fiction",
            "Science fiction picks",
            TemplateCategory::Editorial,
            json!({"genre":878}),
        ),
        (
            "family",
            "Family movie night",
            TemplateCategory::Editorial,
            json!({"genre":10751}),
        ),
        (
            "documentary",
            "Documentaries",
            TemplateCategory::Editorial,
            json!({"genre":99}),
        ),
        (
            "custom-discover",
            "Custom TMDB discover",
            TemplateCategory::Custom,
            json!({}),
        ),
    ]
}

fn discover_template(
    id: &str,
    title: &str,
    category: TemplateCategory,
    media_type: MediaType,
    parameters: &Value,
) -> CollectionTemplate {
    let parameters = parameters
        .as_object()
        .map(|object| {
            object
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default();
    CollectionTemplate {
        provenance: TemplateReference {
            id: format!("tmdb.discover.{}.{}", media_type.identity_name(), id),
            version: 1,
        },
        title: title.to_string(),
        description: String::new(),
        category,
        pictogram: discover_pictogram(id),
        source: CollectionSource::TmdbDiscover {
            schema_version: 1,
            parameters,
        },
        media_type,
        limit: ResultLimit::All,
        ordering: ResultOrdering::Source,
        cadence: RefreshCadence::Daily,
    }
}

fn discover_pictogram(id: &str) -> TemplatePictogram {
    match id {
        "trending-day" => TemplatePictogram::Flame,
        "trending-week" => TemplatePictogram::TrendingUp,
        "popular" => TemplatePictogram::Star,
        "popular-new" => TemplatePictogram::Sparkles,
        "netflix" | "disney-plus" | "prime-video" | "apple-tv" | "max" => {
            TemplatePictogram::MonitorPlay
        }
        "top-rated" => TemplatePictogram::Trophy,
        "critics" => TemplatePictogram::Award,
        "in-theaters" => TemplatePictogram::Popcorn,
        "opening-soon" => TemplatePictogram::CalendarClock,
        "upcoming" => TemplatePictogram::CalendarDays,
        "anticipated" => TemplatePictogram::Rocket,
        "action" => TemplatePictogram::Zap,
        "comedy" => TemplatePictogram::Laugh,
        "drama" => TemplatePictogram::Drama,
        "horror" => TemplatePictogram::Ghost,
        "science-fiction" => TemplatePictogram::Telescope,
        "family" => TemplatePictogram::UsersRound,
        "documentary" => TemplatePictogram::BookOpen,
        "custom-discover" => TemplatePictogram::SlidersHorizontal,
        "adventure" => TemplatePictogram::Compass,
        "animation" => TemplatePictogram::Palette,
        "crime" | "mystery" => TemplatePictogram::Search,
        "fantasy" => TemplatePictogram::Sparkles,
        "history" => TemplatePictogram::Landmark,
        "music" => TemplatePictogram::Music,
        "romance" => TemplatePictogram::Heart,
        "thriller" => TemplatePictogram::Crosshair,
        "war" => TemplatePictogram::Swords,
        "western" => TemplatePictogram::Mountain,
        "tv-movies" => TemplatePictogram::Tv,
        "french-language" | "german-language" | "spanish-language" | "japanese-language"
        | "korean-language" | "hindi-language" => TemplatePictogram::Languages,
        _ if id.starts_with("year-") => TemplatePictogram::CalendarDays,
        _ => TemplatePictogram::Film,
    }
}

fn series_counterpart(mut template: CollectionTemplate) -> CollectionTemplate {
    template.provenance.id = template.provenance.id.replacen(".movie.", ".series.", 1);
    template.title = template.title.replace("movies", "series");
    template.media_type = MediaType::Series;
    template.category = match template.category {
        TemplateCategory::InTheaters => {
            if let CollectionSource::TmdbDiscover { parameters, .. } = &mut template.source {
                let window = parameters
                    .get("releaseWindow")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                parameters.insert(
                    "releaseWindow".to_string(),
                    json!(if window == "opening-soon" {
                        "airing-soon"
                    } else {
                        "on-air"
                    }),
                );
            }
            template.title = template
                .title
                .replace("theaters", "on air")
                .replace("Opening", "Starting");
            TemplateCategory::OnAir
        }
        TemplateCategory::Upcoming => TemplateCategory::OnAir,
        category => category,
    };
    template
}

fn franchise_templates() -> Vec<CollectionTemplate> {
    [
        ("star-wars", "Star Wars", 10, TemplatePictogram::Orbit),
        (
            "james-bond",
            "James Bond",
            645,
            TemplatePictogram::Crosshair,
        ),
        ("alien", "Alien", 8091, TemplatePictogram::Bug),
        ("matrix", "The Matrix", 2344, TemplatePictogram::Binary),
        ("toy-story", "Toy Story", 10194, TemplatePictogram::Blocks),
        (
            "jurassic-park",
            "Jurassic Park",
            328,
            TemplatePictogram::Bone,
        ),
        (
            "mission-impossible",
            "Mission: Impossible",
            87359,
            TemplatePictogram::Briefcase,
        ),
        (
            "lord-of-the-rings",
            "The Lord of the Rings",
            119,
            TemplatePictogram::Circle,
        ),
        (
            "harry-potter",
            "Harry Potter",
            1241,
            TemplatePictogram::WandSparkles,
        ),
        (
            "planet-of-the-apes",
            "Planet of the Apes",
            173710,
            TemplatePictogram::PawPrint,
        ),
    ]
    .into_iter()
    .map(|(id, title, collection_id, pictogram)| CollectionTemplate {
        provenance: TemplateReference {
            id: format!("tmdb.collection.{id}"),
            version: 1,
        },
        title: title.to_string(),
        description: String::new(),
        category: TemplateCategory::Editorial,
        pictogram,
        source: CollectionSource::TmdbCollection {
            schema_version: 1,
            collection_id,
            include_unreleased: false,
        },
        media_type: MediaType::Movie,
        limit: ResultLimit::All,
        ordering: ResultOrdering::Source,
        cadence: RefreshCadence::Weekly,
    })
    .collect()
}

fn discover_variants() -> Vec<CollectionTemplate> {
    let genres = [
        ("adventure", "Adventure", json!({"genre":12})),
        ("animation", "Animation", json!({"genre":16})),
        ("crime", "Crime", json!({"genre":80})),
        ("fantasy", "Fantasy", json!({"genre":14})),
        ("history", "History", json!({"genre":36})),
        ("music", "Music", json!({"genre":10402})),
        ("mystery", "Mystery", json!({"genre":9648})),
        ("romance", "Romance", json!({"genre":10749})),
        ("thriller", "Thriller", json!({"genre":53})),
        ("war", "War", json!({"genre":10752})),
        ("western", "Western", json!({"genre":37})),
        ("tv-movies", "TV movies", json!({"genre":10770})),
        (
            "french-language",
            "French-language films",
            json!({"originalLanguage":"fr"}),
        ),
        (
            "german-language",
            "German-language films",
            json!({"originalLanguage":"de"}),
        ),
        (
            "spanish-language",
            "Spanish-language films",
            json!({"originalLanguage":"es"}),
        ),
        (
            "japanese-language",
            "Japanese-language films",
            json!({"originalLanguage":"ja"}),
        ),
        (
            "korean-language",
            "Korean-language films",
            json!({"originalLanguage":"ko"}),
        ),
        (
            "hindi-language",
            "Hindi-language films",
            json!({"originalLanguage":"hi"}),
        ),
    ];
    let mut variants = Vec::with_capacity(54);
    for (id, title, parameters) in genres {
        variants.push(discover_template(
            id,
            title,
            TemplateCategory::Editorial,
            MediaType::Movie,
            &parameters,
        ));
    }
    for index in 0..36 {
        let year = 2025 - index;
        variants.push(discover_template(
            &format!("year-{year}"),
            &format!("Movies from {year}"),
            TemplateCategory::Editorial,
            MediaType::Movie,
            &json!({"primaryReleaseYear":year}),
        ));
    }
    variants
}

fn mdblist_selectors() -> Vec<CollectionTemplate> {
    (1..=12)
        .map(|index| CollectionTemplate {
            provenance: TemplateReference {
                id: format!("mdblist.public-list.{index:02}"),
                version: 1,
            },
            title: format!("MDBList public list {index}"),
            description: "Choose a public MDBList list.".to_string(),
            category: if index == 12 {
                TemplateCategory::Custom
            } else {
                TemplateCategory::Editorial
            },
            pictogram: TemplatePictogram::ListVideo,
            source: CollectionSource::MdbListPublicList {
                schema_version: 1,
                list_id: "configure".to_string(),
            },
            media_type: MediaType::Mixed,
            limit: ResultLimit::All,
            ordering: ResultOrdering::Source,
            cadence: RefreshCadence::Daily,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn packaged_manifest_has_99_baselines_and_23_series_counterparts() {
        let catalog = catalog();
        assert_eq!(catalog.len(), 122);
        assert_eq!(
            catalog
                .iter()
                .filter(|template| template.provenance.id.starts_with("tmdb.discover.series."))
                .count(),
            23
        );
        let ids = catalog
            .iter()
            .map(|template| template.provenance.id.as_str())
            .collect::<HashSet<_>>();
        assert_eq!(ids.len(), catalog.len());
        assert_eq!(TemplateCategory::ORDER.len(), 9);
    }

    #[test]
    fn packaged_templates_have_contextual_pictograms() {
        let catalog = catalog();
        assert!(
            catalog
                .iter()
                .all(|template| template.pictogram != TemplatePictogram::Film)
        );
        let pictogram = |id: &str| {
            catalog
                .iter()
                .find(|template| template.provenance.id == id)
                .map(|template| template.pictogram)
        };

        assert_eq!(
            pictogram("tmdb.discover.movie.popular"),
            Some(TemplatePictogram::Star)
        );
        assert_eq!(
            pictogram("tmdb.discover.series.horror"),
            Some(TemplatePictogram::Ghost)
        );
        assert_eq!(
            pictogram("tmdb.collection.star-wars"),
            Some(TemplatePictogram::Orbit)
        );
        assert_eq!(
            pictogram("mdblist.public-list.01"),
            Some(TemplatePictogram::ListVideo)
        );
    }
}
