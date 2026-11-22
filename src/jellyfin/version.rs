use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, ts_rs::TS)]
#[ts(type = "string")]
pub struct ServerVersion {
    major: u32,
    minor: u32,
    patch: u32,
}

impl ServerVersion {
    pub const DESKTOP_MINIMUM: Self = Self::new(10, 11, 0);

    pub const fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        let mut parts = raw.split('.').map(|part| {
            part.bytes()
                .all(|byte| byte.is_ascii_digit())
                .then(|| part.parse::<u32>().ok())
                .flatten()
        });
        let (Some(Some(major)), Some(Some(minor)), Some(Some(patch)), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return None;
        };
        Some(Self::new(major, minor, patch))
    }

    pub fn short_of(self, minimum: Self) -> Option<ServerTooOld> {
        (self < minimum).then_some(ServerTooOld {
            version: self,
            minimum,
        })
    }

    pub fn deserialize_reported<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<Self>, D::Error> {
        let raw = Option::<String>::deserialize(deserializer)?;
        Ok(raw.as_deref().and_then(Self::parse))
    }
}

impl fmt::Display for ServerVersion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

impl Serialize for ServerVersion {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ts_rs::TS)]
pub struct ServerTooOld {
    pub version: ServerVersion,
    pub minimum: ServerVersion,
}

#[cfg(test)]
mod tests {
    use super::{ServerTooOld, ServerVersion};
    use crate::jellyfin::api::model::PublicSystemInfo;

    fn reported(json: &str) -> Option<ServerVersion> {
        serde_json::from_str::<PublicSystemInfo>(json)
            .expect("a public system info answer")
            .version
    }

    #[test]
    fn real_public_info_answers_carry_their_server_version() {
        let answers = [
            (
                include_str!("../../fixtures/jellyfin/public-info/10.10.7.json"),
                ServerVersion::new(10, 10, 7),
            ),
            (
                include_str!("../../fixtures/jellyfin/public-info/10.11.0.json"),
                ServerVersion::new(10, 11, 0),
            ),
            (
                include_str!("../../fixtures/jellyfin/public-info/10.11.11.json"),
                ServerVersion::new(10, 11, 11),
            ),
            (
                include_str!("../../fixtures/jellyfin/public-info/12.4.0.json"),
                ServerVersion::new(12, 4, 0),
            ),
        ];
        for (json, version) in answers {
            assert_eq!(reported(json), Some(version), "{json}");
        }
    }

    #[test]
    fn an_answer_without_a_readable_version_reports_none() {
        assert_eq!(reported(r#"{"ServerName":"x"}"#), None);
        assert_eq!(reported(r#"{"Version":null}"#), None);
        assert_eq!(reported(r#"{"Version":""}"#), None);
        assert_eq!(reported(r#"{"version":"10.11.11"}"#), None);
    }

    #[test]
    fn desktop_refuses_10_10_and_accepts_10_11_0_and_newer() {
        let minimum = ServerVersion::DESKTOP_MINIMUM;
        assert_eq!(
            ServerVersion::new(10, 10, 7).short_of(minimum),
            Some(ServerTooOld {
                version: ServerVersion::new(10, 10, 7),
                minimum: ServerVersion::new(10, 11, 0),
            })
        );
        for supported in [
            ServerVersion::new(10, 11, 0),
            ServerVersion::new(10, 11, 11),
            ServerVersion::new(12, 2, 0),
            ServerVersion::new(12, 4, 0),
        ] {
            assert_eq!(supported.short_of(minimum), None, "{supported}");
        }
        assert!(ServerVersion::new(10, 9, 11).short_of(minimum).is_some());
        assert!(ServerVersion::new(9, 99, 99).short_of(minimum).is_some());
    }

    #[test]
    fn rejects_versions_jellyfin_never_reports() {
        for raw in [
            "",
            "10",
            "10.11",
            "10.11.0.0",
            "10.11.0-rc1",
            "10.11.0+build",
            "v10.11.0",
            " 10.11.0",
            "10.11.0 ",
            "10..0",
            "10.11.",
            ".11.0",
            "-1.11.0",
            "+10.11.0",
            "10.11.x",
            "10.11.4294967296",
        ] {
            assert_eq!(ServerVersion::parse(raw), None, "{raw:?}");
        }
    }

    #[test]
    fn versions_order_by_their_numbers_and_show_as_reported() {
        const PARTS: [u32; 9] = [0, 1, 2, 9, 10, 11, 12, 99, 100];
        let mut triples = Vec::new();
        for major in PARTS {
            for minor in PARTS {
                for patch in PARTS {
                    triples.push((major, minor, patch));
                }
            }
        }
        let parsed: Vec<_> = triples
            .iter()
            .map(|&(major, minor, patch)| {
                let raw = format!("{major}.{minor}.{patch}");
                let version = ServerVersion::parse(&raw);
                assert_eq!(
                    version.map(|version| version.to_string()),
                    Some(raw.clone()),
                    "{raw}"
                );
                version
            })
            .collect();
        for (left, left_version) in triples.iter().zip(&parsed) {
            for (right, right_version) in triples.iter().zip(&parsed) {
                assert_eq!(
                    left_version.cmp(right_version),
                    left.cmp(right),
                    "{left:?} against {right:?}"
                );
            }
        }
    }
}
