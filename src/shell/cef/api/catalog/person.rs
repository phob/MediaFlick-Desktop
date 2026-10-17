use super::*;

#[derive(Serialize, ts_rs::TS)]
pub(in super::super) struct PersonResolution {
    person: Option<PersonIdentity>,
    candidates: Vec<PersonIdentity>,
    ambiguous: bool,
}

#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
struct PersonIdentity {
    jellyfin_id: String,
    tmdb_id: Option<i64>,
    name: String,
    image_tag: Option<String>,
}

impl PersonResolution {
    fn found(person: PersonIdentity) -> Self {
        Self {
            person: Some(person),
            candidates: Vec::new(),
            ambiguous: false,
        }
    }
}

pub(super) fn query_person_items(
    services: &Arc<Services>,
    person_id: &str,
    request: &ApiRequest,
) -> Result<ItemPage, ApiResponse> {
    let offset = request
        .param("offset")
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    let limit = request
        .param("limit")
        .and_then(|value| value.parse().ok())
        .unwrap_or(60);
    let scope = session_scope(services)?;
    let (client, user_id) = (scope.client(), scope.user_id());
    match items::fetch_person_items(client, user_id, person_id, offset, limit) {
        Ok(response) => Ok(ItemPage {
            items: response.items.iter().map(ItemSummary::from_dto).collect(),
            total: response.total_record_count,
        }),
        Err(error) => Err(scoped_failure(services, &scope, &error)),
    }
}

fn person_identity(dto: &BaseItemDto, fallback_tmdb_id: Option<i64>) -> PersonIdentity {
    PersonIdentity {
        jellyfin_id: dto.id.clone(),
        tmdb_id: dto
            .provider_id("Tmdb")
            .and_then(|id| id.parse::<i64>().ok())
            .filter(|id| *id > 0)
            .or(fallback_tmdb_id),
        name: dto.display_name().to_string(),
        image_tag: dto.primary_image_tag().map(str::to_string),
    }
}

/// Bridges Jellyfin and TMDB person namespaces without ever treating a fuzzy
/// name match as identity. A missing provider id may use one unambiguous exact
/// name; a known conflicting id is always excluded.
pub(super) fn resolve_person(
    services: &Arc<Services>,
    request: &ApiRequest,
) -> Result<PersonResolution, ApiResponse> {
    let jellyfin_id = request.param("jellyfinId");
    let tmdb_id = request
        .param("tmdbId")
        .and_then(|value| value.parse::<i64>().ok())
        .filter(|value| *value > 0);
    let name = request.param("name").unwrap_or_default();
    let scope = session_scope(services)?;
    let (client, user_id) = (scope.client(), scope.user_id());

    if let Some(jellyfin_id) = jellyfin_id {
        return match items::fetch_item(client, user_id, &jellyfin_id) {
            Ok(Some(person)) if person.item_type.as_deref() == Some("Person") => {
                let provider_id = person
                    .provider_id("Tmdb")
                    .and_then(|id| id.parse::<i64>().ok())
                    .filter(|id| *id > 0);
                if tmdb_id.is_some() && provider_id.is_some() && tmdb_id != provider_id {
                    return Err(ApiResponse::error(
                        409,
                        "the Jellyfin and TMDB person ids do not match",
                    ));
                }
                Ok(PersonResolution::found(person_identity(&person, tmdb_id)))
            }
            Ok(Some(_)) => Err(ApiResponse::error(409, "that Jellyfin id is not a person")),
            Ok(None) => Err(ApiResponse::error(
                404,
                "the server has no person with that id",
            )),
            Err(error) => Err(scoped_failure(services, &scope, &error)),
        };
    }

    if name.trim().is_empty() {
        return Err(ApiResponse::error(
            400,
            "a person name is required to resolve that deep link",
        ));
    }
    match items::fetch_people(client, user_id, &name) {
        Ok(response) => {
            let mut seen = HashSet::new();
            let exact = response
                .items
                .into_iter()
                // `/Persons` is already type-scoped. Some older servers omit
                // `Type` in this lightweight response, so require its stable id
                // and exact name rather than rejecting a valid candidate.
                .filter(|person| !person.id.trim().is_empty())
                .filter(|person| person.display_name().eq_ignore_ascii_case(name.trim()))
                .filter(|person| seen.insert(person.id.clone()))
                .filter(|person| {
                    tmdb_id.is_none_or(|id| {
                        person
                            .provider_id("Tmdb")
                            .and_then(|value| value.parse::<i64>().ok())
                            .filter(|value| *value > 0)
                            .is_none_or(|provider_id| provider_id == id)
                    })
                })
                .collect::<Vec<_>>();
            if exact.len() == 1 {
                return Ok(PersonResolution::found(person_identity(&exact[0], tmdb_id)));
            }
            let candidates = exact
                .iter()
                .map(|person| person_identity(person, tmdb_id))
                .collect::<Vec<_>>();
            Ok(PersonResolution {
                person: None,
                ambiguous: candidates.len() > 1,
                candidates,
            })
        }
        Err(error) => Err(scoped_failure(services, &scope, &error)),
    }
}
