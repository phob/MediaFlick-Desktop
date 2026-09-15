"""Exercise the packaged Windows gpu-next renderer without user configuration."""

import argparse
import ctypes as c
from pathlib import Path
import subprocess
import sys
import tempfile
import time


def render_video(library, directory, warp):
    lib = c.CDLL(str(library.resolve()))
    lib.mpv_client_api_version.restype = c.c_ulong
    if lib.mpv_client_api_version() >> 16 != 2:
        raise RuntimeError("Expected libmpv client API major 2")
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
        options = {
            "config": "no", "load-scripts": "no", "osc": "no",
            "input-default-bindings": "no", "input-vo-keyboard": "no",
            "idle": "yes", "force-window": "yes", "window-minimized": "yes",
            "vo": "gpu-next", "gpu-api": "d3d11", "gpu-context": "d3d11",
            "gpu-sw": "yes", "ao": "null", "hwdec": "no", "keep-open": "yes",
            "log-file": str(directory / "mpv.log"), "msg-level": "all=v",
        }
        if warp:
            options["d3d11-warp"] = "yes"
        for name, value in options.items():
            if lib.mpv_set_option_string(handle, name.encode(), value.encode()) < 0:
                raise RuntimeError(f"libmpv rejected startup option: {name}")
        if lib.mpv_initialize(handle) < 0:
            raise RuntimeError("libmpv initialization failed")

        # Local, deterministic video forces shader compilation and presentation;
        # a version query or successful mpv_initialize alone misses GPU failures.
        media = directory / "video.y4m"
        frame = bytes([128]) * (64 * 64 * 3 // 2)
        media.write_bytes(
            b"YUV4MPEG2 W64 H64 F24:1 Ip A1:1 C420jpeg\n"
            + (b"FRAME\n" + frame) * 72
        )
        command = (c.c_char_p * 3)(b"loadfile", str(media).encode(), None)
        if lib.mpv_command(handle, command) < 0:
            raise RuntimeError("libmpv rejected loadfile")
        deadline = time.monotonic() + 20
        while time.monotonic() < deadline:
            position = property_string("time-pos")
            if position and float(position) > 0.5:
                break
            time.sleep(0.05)
        else:
            raise RuntimeError("Packaged renderer did not advance video playback")
        for name, expected in (
            ("current-vo", "gpu-next"),
            ("current-gpu-context", "d3d11"),
            ("vo-configured", "yes"),
            ("video-out-params/w", "64"),
        ):
            actual = property_string(name)
            if actual != expected:
                raise RuntimeError(f"Expected {name}={expected}, got {actual!r}")
    finally:
        lib.mpv_terminate_destroy(handle)


def smoke_test(library, warp):
    # Native crashes and initialization/shutdown hangs must fail the packaging
    # job instead of taking down its runner or leaving it waiting indefinitely.
    with tempfile.TemporaryDirectory(prefix="mediaflick-windows-gpu-") as temp:
        directory = Path(temp)
        command = [
            sys.executable, str(Path(__file__).resolve()), str(library.resolve()),
            "--worker-directory", str(directory),
        ]
        if warp:
            command.append("--warp")
        try:
            subprocess.run(command, check=True, timeout=45, capture_output=True, text=True)
        except (subprocess.CalledProcessError, subprocess.TimeoutExpired) as error:
            if error.stdout:
                print(error.stdout, file=sys.stderr)
            if error.stderr:
                print(error.stderr, file=sys.stderr)
            log = directory / "mpv.log"
            if log.exists():
                print(log.read_text(errors="replace"), file=sys.stderr)
            raise RuntimeError(f"Packaged Windows renderer failed: {error}") from error
    device = "WARP" if warp else "default adapter"
    print(f"Windows gpu-next/D3D11 video rendering and shutdown passed ({device}): {library}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("library", type=Path)
    parser.add_argument("--warp", action="store_true", help="Use the Direct3D software device")
    parser.add_argument("--worker-directory", type=Path, help=argparse.SUPPRESS)
    args = parser.parse_args()
    if args.worker_directory:
        render_video(args.library, args.worker_directory, args.warp)
    else:
        smoke_test(args.library, args.warp)
