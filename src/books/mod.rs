use serde::{Deserialize, Serialize};

mod appearance;
mod open;
#[cfg(test)]
mod tests;

pub use appearance::ReaderAppearance;
pub use open::OpenBooks;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(rename_all = "lowercase")]
pub enum BookFormat {
    Epub,
    Mobi,
    Azw,
    Azw3,
    Pdf,
    Cbz,
    Cbr,
    Cb7,
    Cbt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgressUnits {
    Fraction,
    Page,
}

const FRACTION_TICKS: f64 = 10_000_000.0;
const PAGE_TICKS: i64 = 10_000;
const PDF_SEARCH: usize = 1024;
const EPUB_CONTAINER: &[u8] = b"META-INF/container.xml";

impl BookFormat {
    pub fn from_name(name: &str) -> Option<Self> {
        let extension = name.rsplit_once('.')?.1.to_ascii_lowercase();
        Some(match extension.as_str() {
            "epub" => Self::Epub,
            "mobi" => Self::Mobi,
            "azw" => Self::Azw,
            "azw3" => Self::Azw3,
            "pdf" => Self::Pdf,
            "cbz" => Self::Cbz,
            "cbr" => Self::Cbr,
            "cb7" => Self::Cb7,
            "cbt" => Self::Cbt,
            _ => return None,
        })
    }

    pub fn units(self) -> ProgressUnits {
        match self {
            Self::Epub | Self::Mobi | Self::Azw | Self::Azw3 => ProgressUnits::Fraction,
            Self::Pdf | Self::Cbz | Self::Cbr | Self::Cb7 | Self::Cbt => ProgressUnits::Page,
        }
    }

    pub fn sniff(head: &[u8], tail: &[u8], name: &str) -> Option<Self> {
        let extension = name
            .rsplit_once('.')
            .map(|(_, extension)| extension.to_ascii_lowercase());
        let hint = extension.as_deref();
        if head.starts_with(b"PK\x03\x04") {
            return Some(if is_epub(head, tail) {
                Self::Epub
            } else {
                Self::Cbz
            });
        }
        if head
            .windows(5)
            .take(PDF_SEARCH + 1)
            .any(|window| window == b"%PDF-")
        {
            return Some(Self::Pdf);
        }
        if head.get(60..68) == Some(b"BOOKMOBI") {
            return Some(match hint {
                Some("azw3") => Self::Azw3,
                Some("azw") => Self::Azw,
                Some("mobi") => Self::Mobi,
                _ if mobi_version(head).is_some_and(|version| version >= 8) => Self::Azw3,
                _ => Self::Mobi,
            });
        }
        if head.starts_with(b"Rar!\x1a\x07\x00") || head.starts_with(b"Rar!\x1a\x07\x01\x00") {
            return Some(Self::Cbr);
        }
        if head.starts_with(b"7z\xbc\xaf\x27\x1c") {
            return Some(Self::Cb7);
        }
        if head.get(257..262) == Some(b"ustar") || (hint == Some("cbt") && tar_checksum_holds(head))
        {
            return Some(Self::Cbt);
        }
        None
    }
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

fn is_epub(head: &[u8], tail: &[u8]) -> bool {
    let u16_at = |offset: usize| {
        head.get(offset..offset + 2)
            .map(|bytes| usize::from(u16::from_le_bytes([bytes[0], bytes[1]])))
    };
    let mimetype_first = u16_at(26).zip(u16_at(28)).is_some_and(|(name, extra)| {
        head.get(30..30 + name) == Some(b"mimetype")
            && head
                .get(30 + name + extra..)
                .is_some_and(|data| data.starts_with(b"application/epub+zip"))
    });
    mimetype_first || contains(head, EPUB_CONTAINER) || contains(tail, EPUB_CONTAINER)
}

fn mobi_version(head: &[u8]) -> Option<u32> {
    let u32_at = |offset: usize| {
        head.get(offset..offset + 4)
            .map(|bytes| u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    };
    let record = usize::try_from(u32_at(78)?).ok()?;
    (head.get(record + 16..record + 20) == Some(b"MOBI")).then_some(())?;
    u32_at(record + 36)
}

fn tar_checksum_holds(head: &[u8]) -> bool {
    let Some(header) = head.get(..512) else {
        return false;
    };
    let field = String::from_utf8_lossy(&header[148..156]);
    let Ok(stored) = u32::from_str_radix(field.trim_matches(|c: char| c == '\0' || c == ' '), 8)
    else {
        return false;
    };
    let sum: u32 = header
        .iter()
        .enumerate()
        .map(|(index, byte)| {
            if (148..156).contains(&index) {
                u32::from(b' ')
            } else {
                u32::from(*byte)
            }
        })
        .sum();
    sum == stored
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, ts_rs::TS)]
#[serde(try_from = "f64")]
#[ts(type = "number")]
pub struct Fraction(f64);

impl Fraction {
    pub fn new(value: f64) -> Option<Self> {
        (value.is_finite() && (0.0..=1.0).contains(&value)).then_some(Self(value))
    }

    pub fn get(self) -> f64 {
        self.0
    }
}

impl TryFrom<f64> for Fraction {
    type Error = &'static str;

    fn try_from(value: f64) -> Result<Self, Self::Error> {
        Self::new(value).ok_or("a fraction must be a finite number from 0 to 1")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(try_from = "String")]
#[ts(type = "string")]
pub struct Cfi(String);

const MAX_CFI_LENGTH: usize = 4096;

impl Cfi {
    pub fn new(value: &str) -> Option<Self> {
        (value.len() <= MAX_CFI_LENGTH && value.starts_with("epubcfi(") && value.ends_with(')'))
            .then(|| Self(value.to_string()))
    }
}

impl TryFrom<String> for Cfi {
    type Error = &'static str;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(&value).ok_or("a location must be an epubcfi(...) of at most 4096 bytes")
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ts_rs::TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ReadingLocation {
    Text { cfi: Cfi, fraction: Fraction },
    Page { page: u32, pages: u32 },
}

impl ReadingLocation {
    pub fn jellyfin_ticks(&self) -> i64 {
        match self {
            Self::Text { fraction, .. } => (fraction.get() * FRACTION_TICKS).round() as i64,
            Self::Page { page, .. } => i64::from(*page) * PAGE_TICKS,
        }
    }

    pub fn is_valid(&self) -> bool {
        match self {
            Self::Text { .. } => true,
            Self::Page { page, pages } => page < pages,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SavedPosition {
    pub location: ReadingLocation,
    pub synced_ticks: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, ts_rs::TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum BookResume {
    Start,
    Exact { location: ReadingLocation },
    Fraction { fraction: Fraction },
    Page { page: u32 },
}

pub fn resume(
    saved: Option<&SavedPosition>,
    server_ticks: i64,
    units: ProgressUnits,
) -> BookResume {
    if let Some(saved) = saved.filter(|saved| saved.synced_ticks == server_ticks) {
        return BookResume::Exact {
            location: saved.location.clone(),
        };
    }
    if server_ticks <= 0 {
        return BookResume::Start;
    }
    match units {
        ProgressUnits::Fraction => BookResume::Fraction {
            fraction: Fraction((server_ticks as f64 / FRACTION_TICKS).min(1.0)),
        },
        ProgressUnits::Page => BookResume::Page {
            page: u32::try_from(server_ticks / PAGE_TICKS).unwrap_or(u32::MAX),
        },
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum ReadingState {
    NotStarted,
    InProgress { percent: Option<u8> },
    Finished,
}

pub fn reading_state(
    saved: Option<&SavedPosition>,
    server_ticks: i64,
    played: bool,
    units: ProgressUnits,
    runtime_ticks: Option<i64>,
) -> ReadingState {
    if played {
        return ReadingState::Finished;
    }
    let fraction = match resume(saved, server_ticks, units) {
        BookResume::Start => return ReadingState::NotStarted,
        BookResume::Exact {
            location: ReadingLocation::Text { fraction, .. },
        }
        | BookResume::Fraction { fraction } => Some(fraction.get()),
        BookResume::Exact {
            location: ReadingLocation::Page { page, pages },
        } => Some(f64::from(page + 1) / f64::from(pages)),
        BookResume::Page { page } => runtime_ticks
            .filter(|ticks| *ticks >= PAGE_TICKS)
            .map(|ticks| ((f64::from(page) + 1.0) / (ticks / PAGE_TICKS) as f64).min(1.0)),
    };
    ReadingState::InProgress {
        percent: fraction.map(|fraction| (fraction * 100.0).round().clamp(0.0, 100.0) as u8),
    }
}
