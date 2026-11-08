use std::collections::VecDeque;
use std::sync::{Mutex, MutexGuard, PoisonError};

use super::{ReadingLocation, SavedPosition};

/// How many opened books are remembered; the least recently used goes first.
const REMEMBERED_BOOKS: usize = 32;

/// The books the reader opened under a session generation, least recently
/// used first. Only an opened book's bytes are served and its place written,
/// and each remembers the ticks Jellyfin holds for it as far as this app
/// knows: what the reader saw on opening, then what Jellyfin confirmed.
#[derive(Default)]
pub struct OpenBooks(Mutex<VecDeque<OpenBook>>);

struct OpenBook {
    generation: u64,
    item_id: String,
    server_ticks: i64,
}

impl OpenBooks {
    /// The reader opened a book Jellyfin held at `server_ticks`.
    pub fn open(&self, generation: u64, item_id: &str, server_ticks: i64) {
        let mut books = self.books();
        books.retain(|book| book.generation == generation && book.item_id != item_id);
        if books.len() == REMEMBERED_BOOKS {
            books.pop_front();
        }
        books.push_back(OpenBook {
            generation,
            item_id: item_id.to_string(),
            server_ticks,
        });
    }

    pub fn is_open(&self, generation: u64, item_id: &str) -> bool {
        self.touch(generation, item_id).is_some()
    }

    /// What a save keeps before Jellyfin confirms it: the new place, with
    /// what Jellyfin holds as synced. `None` for a book that is not open.
    pub fn unsynced(
        &self,
        generation: u64,
        item_id: &str,
        location: ReadingLocation,
    ) -> Option<SavedPosition> {
        Some(SavedPosition {
            location,
            synced_ticks: self.touch(generation, item_id)?,
        })
    }

    /// Jellyfin confirmed it holds `ticks` for the book.
    pub fn synced(&self, generation: u64, item_id: &str, ticks: i64) {
        if let Some(book) = self
            .books()
            .iter_mut()
            .find(|book| book.generation == generation && book.item_id == item_id)
        {
            book.server_ticks = ticks;
        }
    }

    /// Marks the book used and returns the ticks it remembers.
    fn touch(&self, generation: u64, item_id: &str) -> Option<i64> {
        let mut books = self.books();
        let index = books
            .iter()
            .position(|book| book.generation == generation && book.item_id == item_id)?;
        let book = books.remove(index)?;
        let ticks = book.server_ticks;
        books.push_back(book);
        drop(books);
        Some(ticks)
    }

    fn books(&self) -> MutexGuard<'_, VecDeque<OpenBook>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
