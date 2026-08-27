use std::io;

use crate::app::ids::random_hex;

use super::{CollectionProfile, CollectionSource};

/// Result-affecting fields are the only fields that require a fresh Preview
/// and a new revision. Presentation edits keep the active snapshot.
pub fn result_configuration_changed(
    previous: &CollectionProfile,
    next: &CollectionProfile,
) -> bool {
    previous.source != next.source
        || previous.media_type != next.media_type
        || previous.limit != next.limit
        || previous.ordering != next.ordering
}

pub fn allocate_profile_id() -> String {
    random_hex(16)
}

pub fn allocate_revision_id() -> String {
    random_hex(16)
}

/// Accepts an MDBList public-list id or canonical HTTPS list URL and returns
/// the stable id stored in profile JSON. Share tokens and arbitrary hosts are
/// rejected at this boundary.
pub fn normalize_mdblist_list_id(value: &str) -> Result<String, &'static str> {
    let value = value.trim();
    if valid_list_id(value) {
        return Ok(value.to_ascii_lowercase());
    }
    let without_scheme = value
        .strip_prefix("https://mdblist.com/lists/")
        .or_else(|| value.strip_prefix("https://www.mdblist.com/lists/"))
        .ok_or("enter a public MDBList list id or canonical URL")?;
    if without_scheme.contains(['?', '#']) || without_scheme.ends_with('/') {
        return Err("enter a canonical MDBList public-list URL");
    }
    let segments = without_scheme.split('/').collect::<Vec<_>>();
    if segments.len() != 2 || !segments.iter().all(|segment| valid_list_segment(segment)) {
        return Err("enter a canonical MDBList public-list URL");
    }
    Ok(segments.join("/").to_ascii_lowercase())
}

pub fn apply_normalized_mdblist_id(source: &mut CollectionSource) -> io::Result<()> {
    let CollectionSource::MdbListPublicList { list_id, .. } = source else {
        return Ok(());
    };
    *list_id = normalize_mdblist_list_id(list_id)
        .map_err(|message| io::Error::new(io::ErrorKind::InvalidInput, message))?;
    Ok(())
}

fn valid_list_id(value: &str) -> bool {
    value.len() <= 160
        && !value.to_ascii_lowercase().contains("share")
        && match value.split('/').collect::<Vec<_>>().as_slice() {
            [single] => valid_list_segment(single),
            [owner, name] => valid_list_segment(owner) && valid_list_segment(name),
            _ => false,
        }
}

fn valid_list_segment(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mdblist_id_and_url_have_one_identity() {
        assert_eq!(
            normalize_mdblist_list_id("alice/42-popular").expect("selector"),
            normalize_mdblist_list_id("https://mdblist.com/lists/alice/42-popular").expect("url")
        );
        assert!(normalize_mdblist_list_id("https://evil.test/lists/42-popular").is_err());
        assert!(normalize_mdblist_list_id("https://mdblist.com/lists/a/42?share=x").is_err());
    }
}
