"""Load the packaged client ABI, verify its feature profile, and decode audio."""

import ctypes as c
from pathlib import Path
import struct
import sys
import tempfile
import time
import wave


def smoke_test(path):
    lib = c.CDLL(str(path.resolve()))
    lib.mpv_client_api_version.restype = c.c_ulong
    if lib.mpv_client_api_version() >> 16 != 2:
        raise RuntimeError("Expected libmpv client API major 2")
    # Require the render API even though this headless check uses null output.
    for symbol in ("mpv_render_context_create", "mpv_render_context_render", "mpv_render_context_free"):
        getattr(lib, symbol)
    lib.mpv_create.restype = c.c_void_p
    lib.mpv_initialize.argtypes = [c.c_void_p]
    lib.mpv_set_option_string.argtypes = [c.c_void_p, c.c_char_p, c.c_char_p]
    lib.mpv_get_property_string.argtypes = [c.c_void_p, c.c_char_p]
    lib.mpv_get_property_string.restype = c.c_void_p
    lib.mpv_command.argtypes = [c.c_void_p, c.POINTER(c.c_char_p)]
    lib.mpv_free.argtypes = [c.c_void_p]
    lib.mpv_terminate_destroy.argtypes = [c.c_void_p]
    handle = lib.mpv_create()
    if not handle:
        raise RuntimeError("mpv_create failed")

    def property_string(name):
        pointer = lib.mpv_get_property_string(handle, name.encode())
        if not pointer:
            return ""
        try:
            return c.string_at(pointer).decode()
        finally:
            lib.mpv_free(pointer)

    try:
        # Include the app's script-related options: disabling Lua must not make
        # its normal startup configuration fail with an unknown option.
        for name, value in {
            "config": "no", "load-scripts": "no", "osc": "no", "idle": "yes",
            "vo": "null", "ao": "null", "keep-open": "yes", "hwdec": "no",
        }.items():
            status = lib.mpv_set_option_string(handle, name.encode(), value.encode())
            # Mirror the app: missing script-disable options are expected when
            # their engines are not compiled. All other errors remain fatal.
            if status == -5 and name in ("load-scripts", "osc") and value == "no":
                continue
            if status < 0:
                raise RuntimeError(f"libmpv rejected startup option: {name}")
        if lib.mpv_initialize(handle) < 0:
            raise RuntimeError("libmpv initialization failed")
        configuration = property_string("mpv-configuration")
        for option in (
            "-Dlibmpv=true", "-Dcplayer=false", "-Dgpl=true", "-Dauto_features=disabled",
            "-Dlua=disabled", "-Djavascript=disabled", "-Dvapoursynth=disabled",
            "-Ddvdnav=disabled", "-Dlibbluray=disabled", "-Dcdda=disabled",
            "-Dgl=enabled", "-Dplain-gl=enabled", "-Dvaapi=enabled",
            "-Dalsa=enabled", "-Dpulse=enabled", "-Dlcms2=enabled",
        ):
            if option not in configuration.split():
                raise RuntimeError(f"Packaged libmpv lacks required build option: {option}")
        decoders = property_string("decoder-list")
        for codec in ("h264", "hevc", "av1", "aac", "ac3", "flac"):
            if codec not in decoders:
                raise RuntimeError(f"Packaged libmpv lacks expected decoder: {codec}")
        avcodec = c.CDLL(str(next(path.parent.glob("libavcodec.so.*")).resolve()))
        avcodec.avcodec_configuration.restype = c.c_char_p
        ffmpeg_configuration = avcodec.avcodec_configuration().decode().split()
        for option in (
            "--disable-autodetect", "--disable-gpl", "--disable-nonfree",
            "--disable-encoders", "--enable-gnutls", "--enable-libdav1d", "--enable-vaapi",
        ):
            if option not in ffmpeg_configuration:
                raise RuntimeError(f"Packaged FFmpeg lacks required build option: {option}")
        if not property_string("libass-version"):
            raise RuntimeError("Packaged libmpv lacks libass subtitle support")
        with tempfile.TemporaryDirectory(prefix="mediaflick-libmpv-smoke-") as temp:
            media = Path(temp) / "tone.wav"
            with wave.open(str(media), "wb") as wav:
                wav.setparams((1, 2, 8000, 0, "NONE", "not compressed"))
                wav.writeframes(struct.pack("<h", 1000) * 16000)
            command = (c.c_char_p * 3)(b"loadfile", str(media).encode(), None)
            if lib.mpv_command(handle, command) < 0:
                raise RuntimeError("libmpv rejected loadfile")
            deadline = time.monotonic() + 15
            while time.monotonic() < deadline:
                position = property_string("time-pos")
                if position and float(position) > 0.1:
                    break
                time.sleep(0.05)
            else:
                raise RuntimeError("Packaged libmpv did not decode and advance playback")
        print(f"libmpv ABI, startup options, render symbols, and audio decoding passed: {path}")
    finally:
        lib.mpv_terminate_destroy(handle)


if __name__ == "__main__":
    smoke_test(Path(sys.argv[1]))
