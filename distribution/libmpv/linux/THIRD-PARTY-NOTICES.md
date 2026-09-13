# Third-party notices for the Linux libmpv runtime

The private `libmpv.so.2` runtime includes mpv 0.41.0 (GPL-2.0-or-later, required
for upstream X11 support), FFmpeg 8.0.1 with GPL-only features disabled
(LGPL-2.1-or-later), plus libplacebo 7.351.0
(LGPL-2.1-or-later) and its fast_float headers (Apache-2.0 or MIT).

Upstream sources:

- https://github.com/mpv-player/mpv
- https://github.com/FFmpeg/FFmpeg
- https://github.com/haasn/libplacebo
- https://github.com/fastfloat/fast_float
- https://github.com/Dav1dde/glad (MIT/Apache-2.0; generated specifications have their own notices)
- https://github.com/pallets/jinja (BSD-3-Clause; build-time code generator)
- https://github.com/pallets/markupsafe (BSD-3-Clause; build-time code generator)

The payload also bundles distribution libraries needed for decoding, subtitles,
font discovery, color management, compression, and GnuTLS HTTPS. glslang and
SPIRV-Tools shader compiler libraries and Vulkan/NVIDIA codec headers are also
recorded even when statically linked or only used during compilation. Exact
binary/build and source package names and versions are recorded in `SYSTEM-PACKAGES.txt`.
Their copyright notices and license texts are in this directory, including the
`common-licenses` texts referenced by Debian/Ubuntu notices.

`SOURCE-REVISIONS.txt` records upstream revisions and the build environment.
The accompanying `mediaflick-libmpv-linux-<arch>-sources.tar.zst` release asset
contains upstream sources, build scripts and configuration, and the exact
distribution source archives and packaging patches for bundled dependencies.

The host supplies libc, C/C++ runtime support, X11, EGL/OpenGL, the Vulkan
loader, VA-API, GPU drivers, and ALSA/PulseAudio interfaces. They are not
redistributed by this libmpv payload; `HOST-LIBRARIES.txt` lists the linked
host interfaces. NVIDIA's CUDA/NVDEC driver libraries are loaded on demand
and remain optional host components, not bundled proprietary libraries.
