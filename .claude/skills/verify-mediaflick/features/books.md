# Books

A book in a Books library (a non-folder Jellyfin `Book`; audiobooks are not books here) has a detail page with `Read`, `Continue at N%` (with `Read from the start`) or `Read again`, and the primary action has the focus. It opens `/read/<id>`, a full-window reader inside the app shell without sidebar or player bar, for EPUB, MOBI, AZW, AZW3, PDF, CBZ, CBR (RAR 4 and 5), CB7 and CBT. Every function works by keyboard or remote keys alone, and pages also turn by mouse: pages, chapters, the menu, Contents, Go to, Appearance, a Keys sheet, and Back, which closes the top panel and then the reader. The exact place is kept per account in `reader.json`; Jellyfin gets `PlaybackPositionTicks` in jellyfin-web's units through `POST /UserItems/{id}/UserData`, and the last page marks the book played.

## Sub-features

- `book-detail`: `/api/item/<id>` answers `layout: "book"` with `reading` (`notStarted`, `inProgress` with `percent`, `finished`). The page states `Not started`, `N% read`, `Started` or `Finished` and offers the matching link. *Proven by `books`.*
- `book-open`: every format opens from the page by Enter. EPUB, MOBI, AZW, AZW3, PDF and CBZ read the file in ranges; CBR, CB7 and CBT download once and decode in libarchive's worker. Comic pages are ordered naturally (`2.PNG` before `10.PNG`). *Proven by `books` for all eleven fixtures.*
- `book-keys`: Right/Left turn pages, Page Down/Up and Space too, Down/Up in paginated books (scroll in Scrolling layout), Next/Previous track and `]`/`[` change chapters (ten pages in a comic), Home/End jump to the ends, Enter, the Menu key or `m` open the menu, `t`/`g`/`a`/`?` open Contents, Go to, Appearance and Keys, `+`/`-` change the text size, `b` returns after a jump, Escape, Backspace, Remote Back and Alt+Left go back one layer. Play/Pause and Stop are never taken. Keys work with the focus inside the book's iframe. *Proven by `books` (CDP keys, letters, and real X11 keys).*
- `book-mouse`: left click on the book is Next page and right click Previous page, chords `Click` and `RightClick` on those bindings in `ui/src/reader/keys.ts` (the Keys sheet lists them; in Scrolling layout they scroll a screen). Every format, fixed pages included. A click on a link, or the click that ends a drag selecting text, does not turn. Right click inside the book never shows the native menu (the page cancels `contextmenu`); on the footer, an open panel and the rest of the app it still does. The Menu key's `contextmenu`, which carries no button, only opens the reader's menu. Clicks do nothing while a panel is open. *Proven by `reader-mouse` (real X11 clicks).*
- `book-type-scale`: reflowable books (EPUB, MOBI, AZW, AZW3) size their text from the reader's CSS size: 100% is 24px on 1920x1048 (a 1080p screen minus the footer), scaled by the smaller of width/1920 and height/1048, at least 16px, rounded to 0.5px. A 4K screen at 100% scaling gives 48px; at 200% scaling it is 1920x1080 CSS px and gives 24px. The text size setting multiplies that. Columns are at most 40em, 34em or 28em of the 100% size (Narrow, Normal, Wide margins) and foliate's top and bottom margin is twice it, so a text size change keeps the column and a window resize rescales both. PDFs, comics and fixed-layout EPUBs are untouched. *Proven by `reader-mouse` at 3840x2160 and 1920x1080.*
- `book-panels`: the menu (Contents, Go to a position, Appearance, Previous/Next chapter, Back to where you jumped from, Keys, Close the book), Contents (nested entries indented; a PDF or comic without contents lists its pages), Go to (a slider in percent, or pages for a fixed-page book; Page Up raises it by ten steps), Appearance (text size, theme, line spacing, margins and layout for reflowable books; fit and theme for fixed pages), applied at once and kept per account. *Proven by `books`.*
- `book-progress`: one writer per account and book, shared by the reader that closed and the one that reopens it, saves about once a second, at once at the end, and on leaving; a book opens only once that writer has nothing left to save, and a closed reader's reports no longer count. Continue restores the exact CFI or page; a position another client wrote wins over a stale local one, and a place Jellyfin refused wins while Jellyfin still holds what it held when the book was opened. *Proven by `books` and `book-progress` (refused and held writes); the other-client case by `book-progress`'s opening and the unit tests in `src/books/tests.rs`.*
- `book-safety`: book documents run in iframes sandboxed without `allow-scripts`; the page refuses WebAssembly and only libarchive's worker may compile it; book bytes come only from `/api/item/<id>/book/file?start=&end=`, which, like the position route, answers 404 for an item the reader has not opened under the current session (`/api/item/<id>/book` opens it, and only for a non-folder `Book`). *Proven by `books` with the script trap and its API checks.*

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

Per book the drive opens the page, presses Enter, turns three pages with Right, moves a chapter with Next track, then by remote keys alone opens the menu (Menu key), Contents (Down, Enter), Go to (Page Up/Down and arrows to the middle, Enter) and Appearance (Right on the first row, Down, Right on Theme), closes them with Back, and checks the place `reader.json` keeps, the ticks Jellyfin holds, Back to the page, Continue at the same place, and Escape. The keyboard pass uses letters, Escape and Backspace; the x11 pass sends real XTEST keys (Return, Right, XF86AudioNext/Prev, Menu, Down, Prior, XF86Back) after a real click into the book, which also turns a page.

The API checks read each fixture's manifest, a 1 KiB range and an unbounded read cut to one 8 MiB chunk, and expect an audiobook to answer `layout: "audio"` and a 404 manifest, and a movie's and the audiobook's bytes and the audiobook's position write to answer 404.

Evidence: `books.json` (per format: the action label, places, Jellyfin ticks against the expected units, panels, bytes fetched before Pride and Prejudice's first page; the script trap; worker URLs; CSP issues; the keyboard and x11 passes) and `<format>-01-reading` to `-07-reading-after` screenshots, plus `keyboard-keys-sheet` and `x11-reading`.

`reader-mouse` (Linux, `--account`, needs `xdotool`, `xwininfo` and `ffmpeg`):

```sh
MEDIAFLICK_VERIFY_SCREEN=3840x2160x24 just verify reader-mouse --account https://jellyfin.example.com --timeout 900
```

It sizes the app window to the whole 3840x2160 screen with `xdotool`, opens `Pride and Prejudice`, moves to the first chapter with visible body text, and reads inside the section document the `html` and a paragraph's font size, the laid-out `column-width` and the renderer's `max-inline-size` and `margin`, at 3840x2160 and again at 1920x1080. Then, back at 4K, a real left click on a line of text must move the saved place forward, a right click must bring it back to the place before the click with no new viewable top-level X window (the native menu), and a drag across a line must select text and leave the place. A right click on the footer must show one more viewable top-level window (the detector's positive control; the menu is a viewable override-redirect window), a left click on the footer outside it must close it, and the book and its place must stay. Escape is not used: under Xvfb no window manager gives the menu the keyboard, and in one run of three Escape also reached the reader and closed the book. A click on the text the drag selected must clear the selection without turning, and the next click must turn. The Menu key must open the reader's menu without turning a page. On `MediaFlick Fixed Layout`, a left click must move one page forward and a right click one back. Evidence: `reader-mouse.json` (every measured size, place, window list and check) and `01-reading-4k` to `06-fixed-layout` display captures.

## Gotchas

- A click on the book turns the page, so a drive that clicks to put the focus in the iframe has moved a page (`books`' x11 pass expects it).
- The app window opens at 1280x720 whatever the Xvfb screen; `xdotool windowsize` resizes it, and with no window manager under Xvfb nothing else moves it.
- CDP's `rawKeyDown` sends no character, so it does not activate a focused link or button; the drive's `enter()` sends a key-down with `\r`.
- A slider follows ARIA: Page Up raises the value and Page Down lowers it.
- The saved place moves again after a text-size change while the book lays itself out; compare places only after they held still (`settled`).
- On a two-column spread foliate names the page after the last contents entry it shows, so a jump to `Section 5.1` can read `Section 5.2`.
- Reaching a fixed book's last page (Contents or Go to included) marks it played, and its page then offers `Read again` instead of `Continue`.
