"""Load the packaged client ABI, verify its feature profile, and decode audio."""

import argparse
import ctypes as c
from pathlib import Path
import struct
import re
import tempfile
import time
import wave


def smoke_test(path, placebo_config, gpu_api=None):
    configuration_header = placebo_config.read_text()
    for feature in ("OPENGL", "VULKAN", "GLSLANG"):
        if not re.search(rf"^#define PL_HAVE_{feature} 1$", configuration_header, re.MULTILINE):
            raise RuntimeError(f"libplacebo was built without {feature}")
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
        options = {
            "config": "no", "load-scripts": "no", "osc": "no", "idle": "yes",
            "vo": "null", "ao": "null", "keep-open": "yes", "hwdec": "no",
        }
        if gpu_api:
            options.update({"vo": "gpu-next", "gpu-api": gpu_api, "gpu-sw": "yes"})
        for name, value in options.items():
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
            "-Dvulkan=enabled", "-Dcuda-hwaccel=enabled", "-Dcuda-interop=enabled",
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
            "--enable-ffnvcodec", "--enable-nvdec", "--disable-nvenc",
        ):
            if option not in ffmpeg_configuration:
                raise RuntimeError(f"Packaged FFmpeg lacks required build option: {option}")
        # Verify compiled decoder hardware configurations, not just flags.
        # These queries need no GPU or proprietary NVIDIA driver installed.
        class HardwareConfig(c.Structure):
            _fields_ = [("pix_fmt", c.c_int), ("methods", c.c_int), ("device_type", c.c_int)]

        avcodec.avcodec_find_decoder_by_name.argtypes = [c.c_char_p]
        avcodec.avcodec_find_decoder_by_name.restype = c.c_void_p
        avcodec.avcodec_get_hw_config.argtypes = [c.c_void_p, c.c_int]
        avcodec.avcodec_get_hw_config.restype = c.POINTER(HardwareConfig)
        avutil = c.CDLL(str(next(path.parent.glob("libavutil.so.*")).resolve()))
        avutil.av_hwdevice_get_type_name.argtypes = [c.c_int]
        avutil.av_hwdevice_get_type_name.restype = c.c_char_p
        for codec in (b"h264", b"hevc", b"av1"):
            decoder = avcodec.avcodec_find_decoder_by_name(codec)
            if not decoder:
                raise RuntimeError(f"Missing decoder: {codec.decode()}")
            devices = set()
            index = 0
            while config := avcodec.avcodec_get_hw_config(decoder, index):
                devices.add(avutil.av_hwdevice_get_type_name(config.contents.device_type))
                index += 1
            if not {b"cuda", b"vaapi"} <= devices:
                raise RuntimeError(f"Missing NVDEC/VA-API support for {codec.decode()}: {devices}")
        if not property_string("libass-version"):
            raise RuntimeError("Packaged libmpv lacks libass subtitle support")
        with tempfile.TemporaryDirectory(prefix="mediaflick-libmpv-smoke-") as temp:
            media = Path(temp) / "tone.wav"
            with wave.open(str(media), "wb") as wav:
                wav.setparams((1, 2, 8000, 0, "NONE", "not compressed"))
                wav.writeframes(struct.pack("<h", 1000) * 16000)
            if gpu_api:
                media = Path(temp) / "video.y4m"
                frame = bytes([128]) * (16 * 16 * 3 // 2)
                media.write_bytes(b"YUV4MPEG2 W16 H16 F24:1 Ip A1:1 C420jpeg\n" +
                                  (b"FRAME\n" + frame) * 48)
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
            if gpu_api and property_string("current-vo") != "gpu-next":
                raise RuntimeError("Video advanced without the requested gpu-next output")
        mode = f"gpu-next/{gpu_api} video rendering" if gpu_api else "headless audio decoding"
        print(f"libmpv ABI, GPU build profile, and {mode} passed: {path}")
    finally:
        lib.mpv_terminate_destroy(handle)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("library", type=Path)
    parser.add_argument("libplacebo_config", type=Path)
    parser.add_argument("--gpu-api", choices=("opengl", "vulkan"), help="Exercise gpu-next on an X11 display")
    args = parser.parse_args()
    smoke_test(args.library, args.libplacebo_config, args.gpu_api)
