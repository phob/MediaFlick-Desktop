# Third-party notices for the Windows libmpv runtime

The bundled `libmpv-2.dll` is assembled from the projects below. Their exact
source revisions are recorded in `SOURCE-REVISIONS.txt`, and their complete
license texts and corresponding sources are included in the separately
published `mediaflick-libmpv-sources.tar.zst` archive.

| Project | License used by this build | Upstream |
| --- | --- | --- |
| mpv (`gpl=false`) | LGPL-2.1-or-later | https://github.com/mpv-player/mpv |
| FFmpeg (GPL features disabled) | LGPL-2.1-or-later | https://github.com/FFmpeg/FFmpeg |
| libplacebo | LGPL-2.1-or-later | https://code.videolan.org/videolan/libplacebo |
| libass | ISC | https://github.com/libass/libass |
| FriBidi | LGPL-2.1-or-later | https://github.com/fribidi/fribidi |
| Little CMS | MIT | https://github.com/mm2/Little-CMS |
| dav1d | BSD-2-Clause | https://code.videolan.org/videolan/dav1d |
| SPIRV-Cross | Apache-2.0 | https://github.com/KhronosGroup/SPIRV-Cross |
| Vulkan-Headers (headers only) | Apache-2.0 | https://github.com/KhronosGroup/Vulkan-Headers |
| shaderc | Apache-2.0 | https://github.com/google/shaderc |
| glslang | BSD-3-Clause | https://github.com/KhronosGroup/glslang |
| SPIRV-Tools | Apache-2.0 | https://github.com/KhronosGroup/SPIRV-Tools |
| SPIRV-Headers | MIT | https://github.com/KhronosGroup/SPIRV-Headers |
| FreeType | FreeType License or GPL-2.0-only | https://gitlab.freedesktop.org/freetype/freetype |
| HarfBuzz | MIT | https://github.com/harfbuzz/harfbuzz |
| GNU libiconv | LGPL-2.1-or-later | https://www.gnu.org/software/libiconv/ |
| zlib-ng | Zlib | https://github.com/zlib-ng/zlib-ng |
| libpng | Libpng-2.0 | https://github.com/pnggroup/libpng |
| libjpeg-turbo | BSD-3-Clause, IJG, and Zlib | https://github.com/libjpeg-turbo/libjpeg-turbo |
| Brotli | MIT | https://github.com/google/brotli |
| xxHash | BSD-2-Clause | https://github.com/Cyan4973/xxHash |
| glad | MIT and CC0-1.0 generated code | https://github.com/Dav1dde/glad |
| fast_float | Apache-2.0 or MIT | https://github.com/fastfloat/fast_float |
| MinGW-w64/winpthreads | ZPL-2.1 and permissive component licenses | https://www.mingw-w64.org/ |
| GCC runtime libraries | GPL-3.0-or-later with GCC Runtime Library Exception | https://gcc.gnu.org/ |

Windows Schannel and the D3D/OpenGL system interfaces are provided by the
operating system and are not redistributed in this runtime.
