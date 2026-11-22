use super::*;

const KIB: usize = 1024;

fn zip_local_header(name: &str, data: &[u8]) -> Vec<u8> {
    let mut bytes = b"PK\x03\x04".to_vec();
    bytes.extend_from_slice(&[20, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    bytes.extend_from_slice(&[0; 4]);
    bytes.extend_from_slice(&(data.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&(data.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&(name.len() as u16).to_le_bytes());
    bytes.extend_from_slice(&[0, 0]);
    bytes.extend_from_slice(name.as_bytes());
    bytes.extend_from_slice(data);
    bytes
}

fn central_entry(name: &str) -> Vec<u8> {
    let mut bytes = b"PK\x01\x02".to_vec();
    bytes.extend_from_slice(&[0; 24]);
    bytes.extend_from_slice(&(name.len() as u16).to_le_bytes());
    bytes.extend_from_slice(&[0; 16]);
    bytes.extend_from_slice(name.as_bytes());
    bytes
}

const END_OF_CENTRAL_DIRECTORY: &[u8] = b"PK\x05\x06\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0";

fn sniffed(head: &[u8], tail: &[u8], name: &str) -> Option<BookFormat> {
    BookFormat::sniff(head, tail, name)
}

#[test]
fn an_epub_with_its_mimetype_first_is_an_epub() {
    let head = zip_local_header("mimetype", b"application/epub+zip");
    assert_eq!(
        sniffed(&head, END_OF_CENTRAL_DIRECTORY, "a.epub"),
        Some(BookFormat::Epub)
    );
    assert_eq!(
        sniffed(&head, END_OF_CENTRAL_DIRECTORY, "a"),
        Some(BookFormat::Epub)
    );
}

#[test]
fn an_epub_without_a_leading_mimetype_is_found_by_its_container() {
    let mut head = zip_local_header("OEBPS/content.opf", b"<package/>");
    head.extend(zip_local_header("META-INF/container.xml", b"<container/>"));
    assert_eq!(
        sniffed(&head, END_OF_CENTRAL_DIRECTORY, "a.cbz"),
        Some(BookFormat::Epub)
    );

    let head = zip_local_header("OEBPS/content.opf", b"<package/>");
    let mut tail = central_entry("OEBPS/content.opf");
    tail.extend(central_entry("META-INF/container.xml"));
    tail.extend_from_slice(END_OF_CENTRAL_DIRECTORY);
    assert_eq!(sniffed(&head, &tail, "a"), Some(BookFormat::Epub));
}

#[test]
fn a_zip_of_images_is_a_comic_whatever_its_name() {
    let head = zip_local_header("page01.jpg", &[0xff, 0xd8, 0xff]);
    let mut tail = central_entry("page01.jpg");
    tail.extend_from_slice(END_OF_CENTRAL_DIRECTORY);
    assert_eq!(sniffed(&head, &tail, "a.cbz"), Some(BookFormat::Cbz));
    assert_eq!(sniffed(&head, &tail, "a.epub"), Some(BookFormat::Cbz));
}

#[test]
fn an_empty_zip_is_not_a_book() {
    assert_eq!(
        sniffed(END_OF_CENTRAL_DIRECTORY, END_OF_CENTRAL_DIRECTORY, "a.cbz"),
        None
    );
}

#[test]
fn a_pdf_may_start_after_a_little_junk_but_not_late() {
    assert_eq!(
        sniffed(b"%PDF-1.7\n", b"%%EOF", "a.pdf"),
        Some(BookFormat::Pdf)
    );
    let mut late = vec![b' '; KIB];
    late.extend_from_slice(b"%PDF-1.4\n");
    assert_eq!(sniffed(&late, b"%%EOF", "a.pdf"), Some(BookFormat::Pdf));
    let mut too_late = vec![b' '; KIB + 1];
    too_late.extend_from_slice(b"%PDF-1.4\n");
    assert_eq!(sniffed(&too_late, b"%%EOF", "a.pdf"), None);
}

fn palm_database(kind: &[u8; 8], mobi_version: u32) -> Vec<u8> {
    let mut bytes = vec![0u8; 78];
    bytes[60..68].copy_from_slice(kind);
    bytes[76..78].copy_from_slice(&1u16.to_be_bytes());
    let record = 86u32;
    bytes.extend_from_slice(&record.to_be_bytes());
    bytes.extend_from_slice(&[0; 4]);
    bytes.resize(record as usize, 0);
    bytes.extend_from_slice(&[0; 16]);
    bytes.extend_from_slice(b"MOBI");
    bytes.extend_from_slice(&232u32.to_be_bytes());
    bytes.extend_from_slice(&[0; 12]);
    bytes.extend_from_slice(&mobi_version.to_be_bytes());
    bytes.resize(bytes.len() + 200, 0);
    bytes
}

#[test]
fn mobi_files_keep_the_flavour_their_name_gives() {
    let mobi6 = palm_database(b"BOOKMOBI", 6);
    let kf8 = palm_database(b"BOOKMOBI", 8);
    assert_eq!(sniffed(&mobi6, &[], "a.mobi"), Some(BookFormat::Mobi));
    assert_eq!(sniffed(&mobi6, &[], "a.azw"), Some(BookFormat::Azw));
    assert_eq!(sniffed(&kf8, &[], "a.azw3"), Some(BookFormat::Azw3));
    assert_eq!(sniffed(&mobi6, &[], "a.AZW3"), Some(BookFormat::Azw3));
    assert_eq!(sniffed(&kf8, &[], "a"), Some(BookFormat::Azw3));
    assert_eq!(sniffed(&mobi6, &[], "a"), Some(BookFormat::Mobi));
    assert_eq!(sniffed(&kf8, &[], "a.pdf"), Some(BookFormat::Azw3));
}

#[test]
fn a_palmdoc_is_not_a_book_the_reader_opens() {
    assert_eq!(sniffed(&palm_database(b"TEXtREAd", 0), &[], "a.mobi"), None);
}

#[test]
fn rar_7z_and_tar_archives_are_comics() {
    assert_eq!(
        sniffed(b"Rar!\x1a\x07\x00\x00\x00", &[], "a"),
        Some(BookFormat::Cbr)
    );
    assert_eq!(
        sniffed(b"Rar!\x1a\x07\x01\x00\x00", &[], "a.cbz"),
        Some(BookFormat::Cbr)
    );
    assert_eq!(
        sniffed(b"7z\xbc\xaf\x27\x1c\x00\x04", &[], "a.cbr"),
        Some(BookFormat::Cb7)
    );
}

fn tar_header(name: &str, magic: bool) -> Vec<u8> {
    let mut header = vec![0u8; 512];
    header[..name.len()].copy_from_slice(name.as_bytes());
    header[100..107].copy_from_slice(b"0000644");
    header[124..135].copy_from_slice(b"00000000003");
    header[156] = b'0';
    if magic {
        header[257..263].copy_from_slice(b"ustar\0");
        header[263..265].copy_from_slice(b"00");
    }
    header[148..156].copy_from_slice(b"        ");
    let sum: u32 = header.iter().map(|byte| u32::from(*byte)).sum();
    let field = format!("{sum:06o}\0 ");
    header[148..156].copy_from_slice(field.as_bytes());
    header
}

#[test]
fn tar_archives_are_comics_by_magic_or_by_a_valid_old_header() {
    assert_eq!(
        sniffed(&tar_header("1.PNG", true), &[], "a"),
        Some(BookFormat::Cbt)
    );
    let old = tar_header("1.PNG", false);
    assert_eq!(sniffed(&old, &[], "a.cbt"), Some(BookFormat::Cbt));
    assert_eq!(sniffed(&old, &[], "a.Cbt"), Some(BookFormat::Cbt));
    assert_eq!(sniffed(&old, &[], "a"), None);
    let mut broken = old;
    broken[0] = b'X';
    assert_eq!(sniffed(&broken, &[], "a.cbt"), None);
}

#[test]
fn anything_else_is_not_a_book() {
    assert_eq!(sniffed(b"", b"", "a.epub"), None);
    assert_eq!(sniffed(b"PK\x03", b"", "a.cbz"), None);
    assert_eq!(sniffed(b"Call me Ishmael.", b"", "a.txt"), None);
    assert_eq!(
        sniffed(
            b"<!DOCTYPE html><html><body>502</body></html>",
            b"",
            "a.epub"
        ),
        None
    );
    assert_eq!(
        sniffed(b"<?xml version=\"1.0\"?><FictionBook>", b"", "a.fb2"),
        None
    );
}

#[test]
fn the_bytes_win_over_the_name() {
    assert_eq!(sniffed(b"%PDF-1.7\n", b"", "a.epub"), Some(BookFormat::Pdf));
    let head = zip_local_header("mimetype", b"application/epub+zip");
    assert_eq!(
        sniffed(&head, END_OF_CENTRAL_DIRECTORY, "a.pdf"),
        Some(BookFormat::Epub)
    );
}

fn text(fraction: f64) -> ReadingLocation {
    ReadingLocation::Text {
        cfi: Cfi::new("epubcfi(/6/4!/4/2/1:0)").expect("a valid CFI"),
        fraction: Fraction::new(fraction).expect("a valid fraction"),
    }
}

fn page(page: u32, pages: u32) -> ReadingLocation {
    ReadingLocation::Page { page, pages }
}

#[test]
fn nothing_saved_opens_at_the_start() {
    assert_eq!(resume(None, 0, ProgressUnits::Fraction), BookResume::Start);
    assert_eq!(resume(None, 0, ProgressUnits::Page), BookResume::Start);
}

#[test]
fn only_a_server_position_opens_there_in_its_units() {
    assert_eq!(
        resume(None, 2_500_000, ProgressUnits::Fraction),
        BookResume::Fraction {
            fraction: Fraction::new(0.25).expect("fraction")
        }
    );
    assert_eq!(
        resume(None, 50_000, ProgressUnits::Page),
        BookResume::Page { page: 5 }
    );
}

#[test]
fn the_exact_location_wins_while_the_server_holds_what_was_synced() {
    let saved = SavedPosition {
        location: text(0.25),
        synced_ticks: 2_500_000,
    };
    assert_eq!(
        resume(Some(&saved), 2_500_000, ProgressUnits::Fraction),
        BookResume::Exact {
            location: text(0.25)
        }
    );
    let saved = SavedPosition {
        location: page(5, 30),
        synced_ticks: 50_000,
    };
    assert_eq!(
        resume(Some(&saved), 50_000, ProgressUnits::Page),
        BookResume::Exact {
            location: page(5, 30)
        }
    );
}

#[test]
fn another_client_moving_the_book_wins_even_backwards() {
    let saved = SavedPosition {
        location: text(0.8),
        synced_ticks: 8_000_000,
    };
    assert_eq!(
        resume(Some(&saved), 1_000_000, ProgressUnits::Fraction),
        BookResume::Fraction {
            fraction: Fraction::new(0.1).expect("fraction")
        }
    );
    assert_eq!(
        resume(Some(&saved), 0, ProgressUnits::Fraction),
        BookResume::Start
    );
}

#[test]
fn out_of_range_server_values_stay_valid_positions() {
    assert_eq!(resume(None, -5, ProgressUnits::Fraction), BookResume::Start);
    assert_eq!(
        resume(None, 30_000_000, ProgressUnits::Fraction),
        BookResume::Fraction {
            fraction: Fraction::new(1.0).expect("fraction")
        }
    );
    assert_eq!(
        resume(None, 59_999, ProgressUnits::Page),
        BookResume::Page { page: 5 }
    );
}

#[test]
fn locations_write_jellyfin_web_units() {
    assert_eq!(text(1.0).jellyfin_ticks(), 10_000_000);
    assert_eq!(text(0.333_333_33).jellyfin_ticks(), 3_333_333);
    assert_eq!(text(0.0).jellyfin_ticks(), 0);
    assert_eq!(page(5, 30).jellyfin_ticks(), 50_000);
    assert_eq!(page(0, 30).jellyfin_ticks(), 0);
}

#[test]
fn fractions_outside_the_book_are_refused() {
    assert!(Fraction::new(-0.01).is_none());
    assert!(Fraction::new(1.01).is_none());
    assert!(Fraction::new(f64::NAN).is_none());
    assert!(Fraction::new(f64::INFINITY).is_none());
    assert!(Fraction::new(0.5).is_some());
}

#[test]
fn a_refused_place_wins_while_jellyfin_holds_what_it_held_at_opening() {
    let books = OpenBooks::default();
    books.open(1, "book", 30_000);
    let kept = books.unsynced(1, "book", page(5, 271)).expect("open");
    assert_eq!(
        resume(Some(&kept), 30_000, ProgressUnits::Page),
        BookResume::Exact {
            location: page(5, 271)
        }
    );
    assert_eq!(
        resume(Some(&kept), 70_000, ProgressUnits::Page),
        BookResume::Page { page: 7 }
    );
}

#[test]
fn a_confirmed_write_is_what_jellyfin_holds_from_then_on() {
    let books = OpenBooks::default();
    books.open(1, "book", 30_000);
    books.synced(1, "book", 50_000);
    let kept = books.unsynced(1, "book", page(6, 271)).expect("open");
    assert_eq!(
        resume(Some(&kept), 50_000, ProgressUnits::Page),
        BookResume::Exact {
            location: page(6, 271)
        }
    );
}

#[test]
fn only_a_book_opened_under_this_session_is_open() {
    let books = OpenBooks::default();
    assert!(!books.is_open(1, "book"));
    books.open(1, "book", 0);
    assert!(books.is_open(1, "book"));
    assert!(!books.is_open(1, "another"));
    assert!(!books.is_open(2, "book"));
    assert!(books.unsynced(2, "book", page(0, 1)).is_none());
    books.open(2, "another", 0);
    assert!(
        !books.is_open(1, "book"),
        "a new session forgets the old one's books"
    );
}

#[test]
fn the_record_is_bounded_and_keeps_the_book_being_read() {
    let books = OpenBooks::default();
    books.open(1, "reading", 0);
    books.open(1, "left", 0);
    for index in 0..1000 {
        books.open(1, &format!("book {index}"), 0);
        assert!(books.is_open(1, "reading"));
    }
    assert!(!books.is_open(1, "left"));
    assert!(books.is_open(1, "book 999"));
}
