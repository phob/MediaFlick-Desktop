use std::time::Duration;

use serde_json::{Value, json};

use crate::app::build_info;
use crate::app::urls::{build_query, join_url};

const MDBLIST_BASE_URL: &str = "https://api.mdblist.com";
pub(super) const MAX_BATCH_SIZE: usize = 100;
// MDBList's official reference documents a hard maximum of 200 IDs.
const _: () = assert!(MAX_BATCH_SIZE <= 200);
const HTTP_TIMEOUT: Duration = Duration::from_secs(30);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(8);

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Quota {
    pub(super) limit: Option<i64>,
    pub(super) remaining: Option<i64>,
    pub(super) reset_at: Option<i64>,
    pub(super) retry_after_secs: Option<i64>,
}

impl Quota {
    pub(super) fn retry_at(&self, now: i64) -> Option<i64> {
        self.retry_after_secs
            .map(|seconds| now.saturating_add(seconds.max(1)))
            .or_else(|| {
                (self.remaining == Some(0))
                    .then_some(self.reset_at)
                    .flatten()
            })
    }
}

#[derive(Debug)]
pub(super) struct MdbResponse {
    pub(super) body: Value,
    pub(super) quota: Quota,
}

#[derive(Debug)]
pub(super) enum MdbError {
    Unauthorized(Quota),
    RateLimited(Quota),
    Transport,
    Decode,
    Remote { status: u16, quota: Quota },
}

pub(super) trait MdbTransport: Send + Sync {
    fn validate(&self, api_key: &str) -> Result<MdbResponse, MdbError>;
    fn batch(
        &self,
        api_key: &str,
        provider: &str,
        media_type: &str,
        ids: &[String],
    ) -> Result<MdbResponse, MdbError>;
}

pub(super) struct HttpTransport {
    agent: ureq::Agent,
}

impl HttpTransport {
    pub(super) fn new() -> Self {
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(HTTP_TIMEOUT))
            .timeout_connect(Some(CONNECT_TIMEOUT))
            .http_status_as_error(false)
            .user_agent(format!("mediaflick-desktop/{}", build_info::APP_VERSION))
            .build()
            .into();
        Self { agent }
    }

    fn finish(
        &self,
        mut response: ureq::http::Response<ureq::Body>,
    ) -> Result<MdbResponse, MdbError> {
        let status = response.status().as_u16();
        let headers = response.headers();
        let mut quota = Quota {
            limit: integer_header(headers, "x-ratelimit-limit"),
            remaining: integer_header(headers, "x-ratelimit-remaining"),
            reset_at: integer_header(headers, "x-ratelimit-reset"),
            retry_after_secs: integer_header(headers, "retry-after"),
        };
        let bytes = response
            .body_mut()
            .with_config()
            .limit(8 * 1024 * 1024)
            .read_to_vec()
            .map_err(|_| MdbError::Transport)?;
        let body = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice::<Value>(&bytes).map_err(|_| MdbError::Decode)?
        };
        absorb_body_quota(&mut quota, &body);
        match status {
            200..=299 => Ok(MdbResponse { body, quota }),
            401 | 403 => Err(MdbError::Unauthorized(quota)),
            429 => Err(MdbError::RateLimited(quota)),
            _ => Err(MdbError::Remote { status, quota }),
        }
    }
}

impl MdbTransport for HttpTransport {
    fn validate(&self, api_key: &str) -> Result<MdbResponse, MdbError> {
        let url = api_key_url("/user", api_key);
        let response = self
            .agent
            .get(url)
            .header("Accept", "application/json")
            .call()
            .map_err(|_| MdbError::Transport)?;
        self.finish(response)
    }

    fn batch(
        &self,
        api_key: &str,
        provider: &str,
        media_type: &str,
        ids: &[String],
    ) -> Result<MdbResponse, MdbError> {
        let path = format!("/{provider}/{media_type}/");
        let url = api_key_url(&path, api_key);
        let body_ids = ids
            .iter()
            .map(|id| {
                if provider == "tmdb" {
                    id.parse::<i64>().map_or_else(|_| json!(id), Value::from)
                } else {
                    json!(id)
                }
            })
            .collect::<Vec<_>>();
        let response = self
            .agent
            .post(url)
            .header("Accept", "application/json")
            .send_json(json!({ "ids": body_ids }))
            .map_err(|_| MdbError::Transport)?;
        self.finish(response)
    }
}

fn api_key_url(path: &str, api_key: &str) -> String {
    let base = join_url(MDBLIST_BASE_URL, path);
    let query = build_query(&[("apikey", api_key.to_string())]);
    format!("{base}?{query}")
}

fn integer_header(headers: &ureq::http::HeaderMap, name: &str) -> Option<i64> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<i64>().ok())
}

fn integer_at(value: &Value, keys: &[&str]) -> Option<i64> {
    keys.iter().find_map(|key| {
        value
            .get(*key)
            .and_then(|value| value.as_i64().or_else(|| value.as_str()?.parse().ok()))
    })
}

fn absorb_body_quota(quota: &mut Quota, body: &Value) {
    let body_quota = body.get("quota").unwrap_or(body);
    quota.limit = quota
        .limit
        .or_else(|| integer_at(body_quota, &["limit", "api_requests", "apiRequests"]));
    quota.remaining = quota.remaining.or_else(|| {
        integer_at(
            body_quota,
            &[
                "remaining",
                "api_requests_remaining",
                "apiRequestsRemaining",
            ],
        )
    });
    if quota.remaining.is_none()
        && let (Some(limit), Some(used)) = (
            quota.limit,
            integer_at(
                body_quota,
                &["api_requests_count", "apiRequestsCount", "used"],
            ),
        )
    {
        quota.remaining = Some(limit.saturating_sub(used).max(0));
    }
    quota.reset_at = quota
        .reset_at
        .or_else(|| integer_at(body_quota, &["reset", "reset_at", "resetAt"]));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_key_authentication_percent_encodes_query_values() {
        assert!(api_key_url("/user", "a b").ends_with("/user?apikey=a%20b"));
    }

    #[test]
    fn quota_body_and_retry_after_are_normalized() {
        let mut quota = Quota {
            retry_after_secs: Some(120),
            ..Quota::default()
        };
        absorb_body_quota(
            &mut quota,
            &json!({ "api_requests": 1000, "api_requests_count": 22 }),
        );
        assert_eq!(quota.limit, Some(1000));
        assert_eq!(quota.remaining, Some(978));
        assert_eq!(quota.retry_at(100), Some(220));
    }
}
