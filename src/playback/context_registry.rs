use std::time::{Duration, Instant};

use super::{PlaybackContext, PlaybackRequest};

const DEFAULT_CONTEXT_TTL: Duration = Duration::from_secs(15 * 60);

#[derive(Debug, Clone)]
struct PendingContext {
    context: PlaybackContext,
    seen_at: Instant,
}

/// Correlates bridge metadata that can arrive before or after a media request.
#[derive(Debug)]
pub struct PlaybackContextRegistry {
    ttl: Duration,
    contexts: Vec<PendingContext>,
}

impl Default for PlaybackContextRegistry {
    fn default() -> Self {
        Self::new(DEFAULT_CONTEXT_TTL)
    }
}

impl PlaybackContextRegistry {
    pub fn new(ttl: Duration) -> Self {
        Self {
            ttl,
            contexts: Vec::new(),
        }
    }

    pub fn remember(&mut self, context: PlaybackContext) {
        self.prune();
        self.contexts.push(PendingContext {
            context,
            seen_at: Instant::now(),
        });
    }

    pub fn merge_best(&mut self, request: &mut PlaybackRequest) -> Option<(u8, PlaybackContext)> {
        self.prune();
        let best = self
            .contexts
            .iter()
            .filter_map(|pending| {
                let score = pending.context.match_score(request);
                (score > 0).then_some((score, pending.seen_at, pending.context.clone()))
            })
            .max_by_key(|(score, seen_at, _)| (*score, *seen_at));
        if let Some((score, _, context)) = best {
            context.merge_into_request(request);
            Some((score, context))
        } else {
            None
        }
    }

    pub fn prune(&mut self) {
        let now = Instant::now();
        self.contexts
            .retain(|pending| now.saturating_duration_since(pending.seen_at) <= self.ttl);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_the_strongest_recent_context() {
        let mut registry = PlaybackContextRegistry::default();
        registry.remember(PlaybackContext {
            item_id: Some("item".to_string()),
            title: Some("weak".to_string()),
            ..Default::default()
        });
        registry.remember(PlaybackContext {
            play_session_id: Some("session".to_string()),
            title: Some("strong".to_string()),
            ..Default::default()
        });
        let mut request = PlaybackRequest {
            item_id: Some("item".to_string()),
            play_session_id: Some("session".to_string()),
            ..Default::default()
        };

        let (score, _) = registry.merge_best(&mut request).expect("matching context");
        assert_eq!(score, 4);
        assert_eq!(request.title.as_deref(), Some("strong"));
    }

    #[test]
    fn rejects_a_context_with_conflicting_identity() {
        let mut registry = PlaybackContextRegistry::default();
        registry.remember(PlaybackContext {
            item_id: Some("other".to_string()),
            play_session_id: Some("session".to_string()),
            ..Default::default()
        });
        let mut request = PlaybackRequest {
            item_id: Some("current".to_string()),
            play_session_id: Some("session".to_string()),
            ..Default::default()
        };

        assert!(registry.merge_best(&mut request).is_none());
    }

    #[test]
    fn expires_old_contexts() {
        let mut registry = PlaybackContextRegistry::new(Duration::ZERO);
        registry.remember(PlaybackContext {
            item_id: Some("item".to_string()),
            ..Default::default()
        });
        std::thread::sleep(Duration::from_millis(1));
        let mut request = PlaybackRequest {
            item_id: Some("item".to_string()),
            ..Default::default()
        };
        assert!(registry.merge_best(&mut request).is_none());
    }
}
