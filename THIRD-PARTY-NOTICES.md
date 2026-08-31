# Third-party notices

MediaFlick Desktop is distributed under GPL-2.0-or-later. See `LICENSE` for the
project license. Third-party components remain under their own licenses.
Release packages include this file, Chromium's generated `CREDITS.html`, and
the notices described below.

## Chromium Embedded Framework

MediaFlick Desktop redistributes Chromium Embedded Framework and Chromium.
The release package includes Chromium's generated `CREDITS.html` with the
license text and attribution for Chromium's bundled components.

Chromium Embedded Framework is distributed under the following license:

> Copyright (c) 2008-2020 Marshall A. Greenblatt. Portions Copyright (c)
> 2006-2009 Google Inc. All rights reserved.
>
> Redistribution and use in source and binary forms, with or without
> modification, are permitted provided that the following conditions are met:
>
> * Redistributions of source code must retain the above copyright notice,
>   this list of conditions and the following disclaimer.
> * Redistributions in binary form must reproduce the above copyright notice,
>   this list of conditions and the following disclaimer in the documentation
>   and/or other materials provided with the distribution.
> * Neither the name of Google Inc. nor the name Chromium Embedded Framework
>   nor the names of its contributors may be used to endorse or promote
>   products derived from this software without specific prior written
>   permission.
>
> THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS"
> AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
> IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
> ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT OWNER OR CONTRIBUTORS BE
> LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
> CONSEQUENTIAL DAMAGES, INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF
> SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
> INTERRUPTION, HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN
> CONTRACT, STRICT LIABILITY, OR TORT, INCLUDING NEGLIGENCE OR OTHERWISE,
> ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
> POSSIBILITY OF SUCH DAMAGE.

CEF source: https://github.com/chromiumembedded/cef

Chromium source: https://source.chromium.org/chromium

## Bundled Windows libmpv runtime

Windows releases include a dynamically loaded `libmpv-2.dll` built with
GPL-only mpv and FFmpeg features disabled. The release payload contains:

- `licenses/libmpv/THIRD-PARTY-NOTICES.md`
- mpv's GPL and LGPL license texts and copyright notice
- exact source revisions for the bundled libraries
- the DLL's SHA-256 digest

Each GitHub release that contains this runtime must also publish the generated
`mediaflick-libmpv-sources.tar.zst` corresponding-source archive. The pinned
build and release requirements are documented in
`packaging/libmpv/windows/README.md`.

## SQLite

MediaFlick Desktop links a bundled SQLite build through `rusqlite`. SQLite is
in the public domain. See https://www.sqlite.org/copyright.html.

## Source dependencies

The Rust and web dependency manifests and lockfiles record the remaining
source dependencies used to build MediaFlick Desktop:

- `Cargo.toml` and `Cargo.lock`
- `ui/package.json` and `ui/pnpm-lock.yaml`
- `plugin/Jellyfin.Plugin.MediaFlick/Jellyfin.Plugin.MediaFlick.csproj`

Their source distributions contain their respective copyright and license
notices. Those licenses apply only to the corresponding third-party
components.
