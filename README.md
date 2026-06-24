<h1 align="center">
  <img src="resources/app-icon.png" alt="MediaFlick Desktop logo" width="240" height="240">
  <br>
</h1>

# MediaFlick Desktop

[![Draft Release](https://github.com/phob/mediaflick-desktop/actions/workflows/draft-release.yml/badge.svg)](https://github.com/phob/mediaflick-desktop/actions/workflows/draft-release.yml)
[![Latest release](https://img.shields.io/github/v/release/phob/mediaflick-desktop?display_name=tag&sort=semver)](https://github.com/phob/mediaflick-desktop/releases/latest)

External mpv playback for Jellyfin.

MediaFlick Desktop opens Jellyfin Web in a desktop CEF window, then hands direct-play media streams to an external `mpv` process instead of playing them inside the browser. It is built for people who want the Jellyfin Web experience while keeping their own mpv setup: `mpv.conf`, scripts, shaders, SVP4 workflows, HDR profiles, input bindings, and other custom playback features.

While mpv is playing, MediaFlick Desktop still reports playstate back to your Jellyfin server so playback starts, progress, watched state, and resume positions continue to work.

## Features

- Jellyfin Web in a native desktop CEF window
- Direct-play streams handed to an external `mpv` process, so your full mpv setup applies (`mpv.conf`, scripts, shaders, profiles, SVP4, custom HDR, input bindings)
- Playstate reported back to Jellyfin: playback start, progress, watched state, and resume positions
- Jellyfin media-segment skipping for intros, credits, recaps, and commercials, with per-type prompt or auto-skip (countdown) settings
- Skip-segment markers on the mpv seek bar so the timeline shows where segments are skipped, merged with the file's own chapters
- Optional MPC-HC player backend on Windows, switchable live from Client Settings (mpv remains the default)
- One-click **Download mpv** on Windows, with auto-detection of a system `mpv` on Linux and macOS
- Automatic in-app updates from GitHub Releases (Windows)
- Cross-platform: Windows, Linux, and macOS

## Why

I’ve always wanted an app that offered the convenience and look of the media player desktop apps provided by developers, but with the ability to enjoy SVP4 and SDR-to-HDR content. Almost all desktop media player apps are partially based on libmpv, without being able to fully utilize all of mpv’s capabilities. While there are the well-known mpv shim applications—which I’ve used for a very long time. Now the new Jellyfin desktop app, currently still in development, came with the promise that it would fully read the mpv configuration and thus be highly customizable. This is true in many respects, but especially when it comes to technologies like integrating SVP 4 and custom HDR profiles, I believe the limitation of having MPV within the app is the main factor behind many of these restrictions. And so I had the idea to simply write a desktop app for myself that exclusively connects to and controls an external MPV player.

## Disclosure

**This Project is AI assisted mainly for getting the linux and mac builds created and to review the code.**
**AI was used to bundle the App and for the github actions**

## Install

Download the latest `MediaFlickDesktop-Setup-<version>.exe` from [GitHub Releases](https://github.com/phob/mediaflick-desktop/releases/latest) and run it.

On Linux and macOS, use a release archive and run `mediaflick-desktop`. mpv is not bundled: Windows offers a one-click **Download mpv**, while Linux and macOS auto-detect a system `mpv`.

## Build it yourself

See [BUILDING.md](BUILDING.md).
