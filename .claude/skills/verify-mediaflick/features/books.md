# Books

A book in a Books library (a non-folder Jellyfin `Book`; audiobooks are not books here) has a detail page with `Read`, `Continue at N%` (with `Read from the start`) or `Read again`, and the primary action has the focus. It opens `/read/<id>`, a full-window reader inside the app shell without sidebar or player bar, for EPUB, MOBI, AZW, AZW3, PDF, CBZ, CBR (RAR 4 and 5), CB7 and CBT. Every function works by keyboard or remote keys alone: pages, chapters, the menu, Contents, Go to, Appearance, a Keys sheet, and Back, which closes the top panel and then the reader. The exact place is kept per account in `reader.json`; Jellyfin gets `PlaybackPositionTicks` in jellyfin-web's units through `POST /UserItems/{id}/UserData`, and the last page marks the book played.

## Sub-features

- `book-detail`: `/api/item/<id>` answers `layout: "book"` with `reading` (`notStarted`, `inProgress` with `percent`, `finished`). The page states `Not started`, `N% read`, `Started` or `Finished` and offers the matching link. *Proven by `books`.*
- `book-open`: every format opens from the page by Enter. EPUB, MOBI, AZW, AZW3, PDF and CBZ read the file in ranges; CBR, CB7 and CBT download once and decode in libarchive's worker. Comic pages are ordered naturally (`2.PNG` before `10.PNG`). *Proven by `books` for all eleven fixtures.*
- `book-keys`: Right/Left turn pages, Page Down/Up and Space too, Down/Up in paginated books (scroll in Scrolling layout), Next/Previous track and `]`/`[` change chapters (ten pages in a comic), Home/End jump to the ends, Enter, the Menu key or `m` open the menu, `t`/`g`/`a`/`?` open Contents, Go to, Appearance and Keys, `+`/`-` change the text size, `b` returns after a jump, Escape, Backspace, Remote Back and Alt+Left go back one layer. Play/Pause and Stop are never taken. Keys work with the focus inside the book's iframe. *Proven by `books` (CDP keys, letters, and real X11 keys).*
- `book-panels`: the menu (Contents, Go to a position, Appearance, Previous/Next chapter, Back to where you jumped from, Keys, Close the book), Contents (nested entries indented; a PDF or comic without contents lists its pages), Go to (a slider in percent, or pages for a fixed-page book; Page Up raises it by ten steps), Appearance (text size, theme, line spacing, margins and layout for reflowable books; fit and theme for fixed pages), applied at once and kept per account. *Proven by `books`.*
- `book-progress`: one writer saves about once a second, at once at the end, and on leaving. Continue restores the exact CFI or page; a position another client wrote wins over a stale local one. *Proven by `books`; the other-client case only by the unit tests in `src/books/tests.rs`.*
- `book-safety`: book documents run in iframes sandboxed without `allow-scripts`; the page refuses WebAssembly and only libarchive's worker may compile it; book bytes come only from `/api/item/<id>/book/file?start=&end=`. *Proven by `books` with the script trap.*

## How to get to it (user POV)

- The sidebar's `Books` library, then a card `Open details for <title>`; Enter on the focused `Read` link.
- Back on the reader returns to the book's page, which focuses `Continue at N%` or `Read again`.

## Driving it with ctx (just verify)

Preconditions: `--account SERVER` whose Books library holds the fixtures below; writing UserData on that account must be acceptable (each book is restored after the run with `ctx.restoreItemAfterRun`).

```sh
just verify books --account https://jellyfin.example.com --timeout 1500
MEDIAFLICK_VERIFY_BOOKS=epub,cbr,keyboard just verify books --account https://jellyfin.example.com
```

`MEDIAFLICK_VERIFY_BOOKS` limits the run to format keys (`epub epub3 epub2 mobi azw azw3 pdf cbz cbt cb7 cbr nested fxl`) and the `keyboard` and `x11` passes; the script trap always runs. Fixtures: the public-domain books named in `drives/books.mjs`, plus three self-authored EPUBs built by `python3 -I .claude/skills/verify-mediaflick/scripts/book-fixtures.py DIR` (deterministic; it prints their SHA-256) and copied into the library: a script trap, a twelve-chapter book with a three-level contents, and an eight-page fixed-layout book.

Per book the drive opens the page, presses Enter, turns three pages with Right, moves a chapter with Next track, then by remote keys alone opens the menu (Menu key), Contents (Down, Enter), Go to (Page Up/Down and arrows to the middle, Enter) and Appearance (Right on the first row, Down, Right on Theme), closes them with Back, and checks the place `reader.json` keeps, the ticks Jellyfin holds, Back to the page, Continue at the same place, and Escape. The keyboard pass uses letters, Escape and Backspace; the x11 pass sends real XTEST keys (Return, Right, XF86AudioNext/Prev, Menu, Down, Prior, XF86Back) after a real click into the book.

Evidence: `books.json` (per format: the action label, places, Jellyfin ticks against the expected units, panels, bytes fetched before Pride and Prejudice's first page; the script trap; worker URLs; CSP issues; the keyboard and x11 passes) and `<format>-01-reading` to `-07-reading-after` screenshots, plus `keyboard-keys-sheet` and `x11-reading`.

## Gotchas

- CDP's `rawKeyDown` sends no character, so it does not activate a focused link or button; the drive's `enter()` sends a key-down with `\r`.
- A slider follows ARIA: Page Up raises the value and Page Down lowers it.
- The saved place moves again after a text-size change while the book lays itself out; compare places only after they held still (`settled`).
- On a two-column spread foliate names the page after the last contents entry it shows, so a jump to `Section 5.1` can read `Section 5.2`.
- Reaching a fixed book's last page (Contents or Go to included) marks it played, and its page then offers `Read again` instead of `Continue`.
