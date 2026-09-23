### Changed

- Background threads now have names, which makes crash reports and debugging easier. The app no longer stops if one of them cannot start; the failure is logged.
- Large responses from the local API, such as artwork and trailer segments, are no longer copied a second time before they are sent.
