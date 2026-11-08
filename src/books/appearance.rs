use serde::{Deserialize, Serialize};

/// How the reader shows books, one setting per account. Text settings apply
/// to reflowable books, `fit` to PDFs, comics and fixed-layout EPUBs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReaderAppearance {
    pub font_size: FontSize,
    pub line_spacing: LineSpacing,
    pub margins: Margins,
    pub theme: ReaderTheme,
    pub flow: ReaderFlow,
    pub fit: PageFit,
}

/// Text size in percent of the book's own, from 80 to 200 in steps of 10.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(try_from = "u16")]
#[ts(type = "number")]
pub struct FontSize(u16);

impl FontSize {
    pub const MIN: u16 = 80;
    pub const MAX: u16 = 200;
    pub const STEP: u16 = 10;
}

impl Default for FontSize {
    fn default() -> Self {
        Self(100)
    }
}

impl TryFrom<u16> for FontSize {
    type Error = &'static str;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        ((Self::MIN..=Self::MAX).contains(&value) && value.is_multiple_of(Self::STEP))
            .then_some(Self(value))
            .ok_or("the text size must be 80 to 200 percent in steps of 10")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub enum LineSpacing {
    Compact,
    #[default]
    Normal,
    Relaxed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub enum Margins {
    Narrow,
    #[default]
    Normal,
    Wide,
}

/// `App` follows the app's own light or dark appearance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub enum ReaderTheme {
    #[default]
    App,
    Light,
    Sepia,
    Dark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub enum ReaderFlow {
    #[default]
    Paginated,
    Scrolled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub enum PageFit {
    #[default]
    Page,
    Width,
}
