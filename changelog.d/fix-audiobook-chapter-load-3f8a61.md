### Fixed

- An audiobook with many chapters (an .m4b) starts in seconds from a remote server. Reading its chapter list made a new connection for every chapter, so a 285-chapter book could take over a minute to open and was abandoned before it played.
