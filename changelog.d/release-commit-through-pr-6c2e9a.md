### Changed

- A release is now prepared in the release-notes pull request: `scripts/prepare-release.py` promotes the changelog and sets the version there, so the release commit passes the same checks as every other change. The Draft Release workflow no longer commits to `main`. Run on `main`, it stops within a minute unless the checkout already is the requested release, then builds it, tags the commit it built, and drafts the release. Test drafts work as before.
