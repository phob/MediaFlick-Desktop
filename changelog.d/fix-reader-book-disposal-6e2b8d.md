### Fixed

- The reader gives back a comic's or PDF's pages as it leaves them, also when the next page key comes before a page has loaded, and frees the whole book when it closes, even while a page is still loading with another page key waiting, so reading a long comic no longer holds every page read, and each PDF opened, even one closed while it was still opening, no longer leaves its decoder running or keeps a little memory for every page read.
