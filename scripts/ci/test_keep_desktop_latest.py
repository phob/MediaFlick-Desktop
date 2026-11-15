"""Tests for the Latest release guard's decision.

The expected values come from the rule Martin set on 2026-11-15, not from the
code: GitHub's Latest release must carry a desktop tag `v<digits>.<digits>.<digits>`,
because the desktop updater (`src/maintenance/updater.rs`) and the flick.media
download buttons both read `releases/latest`. When it does not, Latest goes back
to the newest published, non-prerelease desktop release and the run fails.

Ways the decision could go wrong, each covered below:
- a Companion tag, a suffixed desktop tag or a near miss counts as a desktop tag;
- non-ASCII digits or surrounding whitespace pass the pattern;
- a desktop Latest that is not the newest gets overridden;
- a missing Latest (404) passes silently;
- a draft, a prerelease, a Companion or a suffixed release is chosen to restore;
- "newest" compares tags as text (v0.10.0 < v0.9.0) or by publish order;
- no candidate leads to a crash or a silent pass.
"""

import random
import unittest

from keep_desktop_latest import Keep, NoCandidate, Release, Restore, decide, is_desktop_tag


def release(tag: str, *, id: int = 0, draft: bool = False, prerelease: bool = False) -> Release:
    return Release(id=id, tag=tag, draft=draft, prerelease=prerelease)


DESKTOP = [release("v0.1.5", id=5), release("v0.1.6", id=6)]


class DesktopTagTests(unittest.TestCase):
    def test_desktop_tags(self):
        for tag in ("v0.1.6", "v0.2.0", "v1.0.0", "v10.20.30", "v0.10.0"):
            self.assertTrue(is_desktop_tag(tag), tag)

    def test_everything_else_is_not_a_desktop_tag(self):
        for tag in (
            "companion-v0.2.2.0",
            "companion-v0.2.2",
            "v0.2.0-rc.1",
            "v0.2.0-test.2",
            "v0.2.0+build",
            "v0.2",
            "v0.2.0.0",
            "0.2.0",
            "V0.2.0",
            "v0.2.0 ",
            "v0.2.0\n",
            " v0.2.0",
            "v０.2.0",  # fullwidth digit zero
            "v0.٢.0",  # Arabic-Indic digit two
            "",
        ):
            self.assertFalse(is_desktop_tag(tag), repr(tag))


class DecisionTests(unittest.TestCase):
    def test_desktop_latest_is_kept(self):
        self.assertEqual(decide("v0.1.6", DESKTOP), Keep("v0.1.6"))

    def test_an_older_desktop_latest_is_kept(self):
        # The rule only asks for a desktop tag; an older one may be deliberate.
        releases = [*DESKTOP, release("v0.2.0", id=20)]
        self.assertEqual(decide("v0.1.6", releases), Keep("v0.1.6"))

    def test_companion_latest_is_restored_to_the_newest_desktop_release(self):
        releases = [release("companion-v0.2.2.0", id=99), *DESKTOP]
        self.assertEqual(decide("companion-v0.2.2.0", releases), Restore("companion-v0.2.2.0", DESKTOP[1]))

    def test_suffixed_desktop_latest_is_restored(self):
        releases = [release("v0.2.0-rc.1", id=30), *DESKTOP]
        self.assertEqual(decide("v0.2.0-rc.1", releases), Restore("v0.2.0-rc.1", DESKTOP[1]))

    def test_no_latest_is_restored(self):
        self.assertEqual(decide(None, DESKTOP), Restore(None, DESKTOP[1]))

    def test_candidates_skip_drafts_prereleases_and_other_tags(self):
        releases = [
            release("v9.0.0", id=1, draft=True),
            release("v8.0.0", id=2, prerelease=True),
            release("v7.0.0-rc.1", id=3),
            release("companion-v6.0.0", id=4),
            release("v6.0.0.1", id=5),
            release("v0.3.0", id=6),
        ]
        self.assertEqual(decide("companion-v6.0.0", releases), Restore("companion-v6.0.0", releases[5]))

    def test_newest_compares_numbers_not_text_or_order(self):
        older = release("v0.9.9", id=1)
        newer = release("v0.10.0", id=2)
        hotfix = release("v0.1.7", id=3)
        for order in ([older, newer, hotfix], [hotfix, newer, older], [newer, hotfix, older]):
            self.assertEqual(decide("companion-v1.0.0", order), Restore("companion-v1.0.0", newer))

    def test_no_candidate(self):
        releases = [release("companion-v0.2.2.0", id=1), release("v0.2.0", id=2, prerelease=True)]
        self.assertEqual(decide("companion-v0.2.2.0", releases), NoCandidate("companion-v0.2.2.0"))
        self.assertEqual(decide(None, []), NoCandidate(None))

    def test_restore_target_is_always_the_highest_stable_desktop_release(self):
        # Invariant over generated release lists: whatever the mix, a non-desktop
        # Latest is restored to the highest x.y.z among published, non-prerelease
        # desktop releases, and a desktop Latest is kept.
        generator = random.Random(20261115)
        for _ in range(500):
            releases = []
            for index in range(generator.randint(0, 12)):
                numbers = [generator.randint(0, 12) for _ in range(3)]
                tag = generator.choice(["v{}.{}.{}", "companion-v{}.{}.{}.0", "v{}.{}.{}-rc.1"]).format(*numbers)
                releases.append(release(tag, id=index, draft=generator.random() < 0.2, prerelease=generator.random() < 0.2))
            stable = [
                (tuple(int(part) for part in r.tag[1:].split(".")), r)
                for r in releases
                if r.tag.startswith("v") and "-" not in r.tag and not r.draft and not r.prerelease
            ]
            best = max(stable, key=lambda pair: pair[0])[1] if stable else None
            latest = generator.choice([None, *(r.tag for r in releases)])
            decision = decide(latest, releases)
            if latest is not None and latest.startswith("v") and "-" not in latest:
                self.assertEqual(decision, Keep(latest))
            elif best is None:
                self.assertEqual(decision, NoCandidate(latest))
            else:
                self.assertIsInstance(decision, Restore)
                assert isinstance(decision, Restore)  # narrows the type for the next line
                self.assertEqual(decision.target.tag, best.tag)


if __name__ == "__main__":
    unittest.main()
