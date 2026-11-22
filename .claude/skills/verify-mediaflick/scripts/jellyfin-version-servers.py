#!/usr/bin/env python3
from __future__ import annotations

import importlib.util
import json
import platform
import shutil
import signal
import socket
import subprocess
import sys
import tarfile
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[4]
CACHE = ROOT / "build/e2e/jellyfin-servers"
OUTPUT = ROOT / "build/e2e/jellyfin-version-servers"
SOURCES = (
    "https://repo.jellyfin.org/files/server/linux/stable/v{version}/{arch}/jellyfin_{version}-{arch}.tar.xz",
    "https://repo.jellyfin.org/archive/server/linux/stable/v{version}/{arch}/jellyfin_{version}-{arch}.tar.xz",
)
CLIENT = 'MediaBrowser Client="MediaFlick E2E", Device="e2e", DeviceId="mediaflick-version-servers", Version="1.0.0"'
USER, PASSWORD = "e2e-admin", "e2e-password"
LIBRARIES = (
    ("Movies", "movies", "Movies/Test Movie (2020)/Test Movie (2020).mp4", 60),
    ("Shows", "tvshows", "Shows/Test Show/Season 01/Test Show S01E01.mp4", 30),
    ("Shows", "tvshows", "Shows/Test Show/Season 01/Test Show S01E02.mp4", 30),
)


def fetch(version: str) -> Path:
    arch = {"x86_64": "amd64", "aarch64": "arm64"}[platform.machine()]
    target = CACHE / f"{version}-{arch}"
    if (target / "jellyfin/jellyfin").is_file():
        return target / "jellyfin/jellyfin"
    CACHE.mkdir(parents=True, exist_ok=True)
    for source in SOURCES:
        url = source.format(version=version, arch=arch)
        try:
            with urllib.request.urlopen(url, timeout=300) as reply, tempfile.TemporaryFile() as archive:
                shutil.copyfileobj(reply, archive)
                archive.seek(0)
                with tarfile.open(fileobj=archive, mode="r:xz") as tar:
                    tar.extractall(target, filter="data")
            return target / "jellyfin/jellyfin"
        except urllib.error.HTTPError as error:
            if error.code != 404:
                raise
    raise SystemExit(f"Jellyfin {version} was not found at repo.jellyfin.org")


def media(root: Path) -> None:
    for _, _, relative, seconds in LIBRARIES:
        file = root / relative
        if file.exists():
            continue
        file.parent.mkdir(parents=True, exist_ok=True)
        subprocess.run(["ffmpeg", "-loglevel", "error", "-y", "-f", "lavfi", "-i", "testsrc2=size=1280x720:rate=24",
                        "-f", "lavfi", "-i", "sine=frequency=440", "-t", str(seconds), "-c:v", "libx264",
                        "-preset", "veryfast", "-pix_fmt", "yuv420p", "-c:a", "aac", "-shortest", str(file)], check=True)


def request(base: str, method: str, path: str, token: str | None = None, body: object = None) -> tuple[int, str]:
    headers = {"Authorization": CLIENT if token is None else f'{CLIENT}, Token="{token}"', "Accept": "application/json"}
    data = None if body is None else json.dumps(body).encode()
    if data is not None:
        headers["Content-Type"] = "application/json"
    try:
        with urllib.request.urlopen(urllib.request.Request(base + path, data, headers, method=method), timeout=120) as reply:
            return reply.status, reply.read().decode()
    except urllib.error.HTTPError as error:
        return error.code, error.read().decode("utf-8", "replace")


def ok(answer: tuple[int, str], step: str) -> str:
    if not 200 <= answer[0] < 300:
        raise SystemExit(f"{step}: HTTP {answer[0]} {answer[1][:300]}")
    return answer[1]


def free_port() -> int:
    with socket.socket() as probe:
        probe.bind(("127.0.0.1", 0))
        return probe.getsockname()[1]


def companion_package() -> Path:
    spec = importlib.util.spec_from_file_location("companion_jellyfin", ROOT / "scripts/e2e/companion-jellyfin.py")
    if spec is None or spec.loader is None:
        raise SystemExit("cannot load scripts/e2e/companion-jellyfin.py")
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    work = OUTPUT / "companion-package"
    shutil.rmtree(work, ignore_errors=True)
    work.mkdir(parents=True)
    archive, _ = module.build_package(work)
    return archive


def start(spec: str, executable: Path, media_root: Path, companion: Path | None) -> tuple[subprocess.Popen[bytes], dict[str, object]]:
    version = spec.removesuffix("+companion")
    port = free_port()
    data = OUTPUT / spec
    shutil.rmtree(data, ignore_errors=True)
    config = data / "config"
    config.mkdir(parents=True)
    if companion is not None:
        with zipfile.ZipFile(companion) as package:
            meta = json.loads(package.read("meta.json"))
            package.extractall(data / "data" / "plugins" / f"{meta['name']}_{meta['version']}")
    (config / "network.xml").write_text(
        f'<?xml version="1.0" encoding="utf-8"?>\n<NetworkConfiguration><InternalHttpPort>{port}</InternalHttpPort>'
        f"<PublicHttpPort>{port}</PublicHttpPort><EnableHttps>false</EnableHttps><AutoDiscovery>false</AutoDiscovery>"
        "<EnableUPnP>false</EnableUPnP></NetworkConfiguration>\n")
    command = [str(executable), "--datadir", str(data / "data"), "--configdir", str(config), "--cachedir", str(data / "cache"),
               "--logdir", str(data / "log"), "--nowebclient"]
    if shutil.which("ffmpeg"):
        command += ["--ffmpeg", shutil.which("ffmpeg")]
    process = subprocess.Popen(command, stdout=(data / "console.log").open("wb"), stderr=subprocess.STDOUT)
    try:
        base = f"http://127.0.0.1:{port}"
        deadline = time.monotonic() + 180
        while True:
            if process.poll() is not None or time.monotonic() > deadline:
                raise SystemExit(f"Jellyfin {version} did not start; see {data / 'console.log'}")
            try:
                status, body = request(base, "GET", "/System/Info/Public")
                if status == 200 and json.loads(body).get("Version"):
                    break
            except (urllib.error.URLError, ConnectionError, TimeoutError, json.JSONDecodeError):
                pass
            time.sleep(1)
        info = json.loads(body)
        if info["Version"] != version and not info["Version"].startswith(f"{version}."):
            raise SystemExit(f"the {version} build reports {info['Version']}")
        (data / "public-info.json").write_text(body)
        ok(request(base, "POST", "/Startup/Configuration", body={"UICulture": "en-US", "MetadataCountryCode": "US", "PreferredMetadataLanguage": "en"}), "startup configuration")
        ok(request(base, "GET", "/Startup/User"), "startup user lookup")
        ok(request(base, "POST", "/Startup/User", body={"Name": USER, "Password": PASSWORD}), "startup user")
        ok(request(base, "POST", "/Startup/Complete"), "startup completion")
        token = json.loads(ok(request(base, "POST", "/Users/AuthenticateByName", body={"Username": USER, "Pw": PASSWORD}), "sign-in"))["AccessToken"]
        for name, kind in dict.fromkeys((name, kind) for name, kind, _, _ in LIBRARIES):
            query = urllib.parse.urlencode({"name": name, "collectionType": kind, "refreshLibrary": "true"})
            options = {"LibraryOptions": {"PathInfos": [{"Path": str(media_root / name)}]}}
            ok(request(base, "POST", f"/Library/VirtualFolders?{query}", token, options), f"library {name}")
        rescan = 0.0
        while json.loads(ok(request(base, "GET", "/Items?Recursive=true&IncludeItemTypes=Movie,Episode", token), "items"))["TotalRecordCount"] < len(LIBRARIES):
            if time.monotonic() > deadline:
                raise SystemExit(f"Jellyfin {version} did not scan the test libraries")
            if time.monotonic() > rescan:
                ok(request(base, "POST", "/Library/Refresh", token), "library scan")
                rescan = time.monotonic() + 20
            time.sleep(1)
        if companion is not None:
            ok(request(base, "GET", "/MediaFlick/info", token), f"the Companion on Jellyfin {info['Version']}")
        request(base, "POST", "/Sessions/Logout", token)
        return process, {"version": info["Version"], "requested": spec, "url": base, "username": USER, "password": PASSWORD,
                         "companion": companion is not None}
    except BaseException:
        process.terminate()
        process.wait(timeout=30)
        raise


def main() -> int:
    versions = sys.argv[1:] or ["10.10.7", "10.11.0", "10.11.11"]
    OUTPUT.mkdir(parents=True, exist_ok=True)
    media_root = OUTPUT / "media"
    media(media_root)
    processes = []
    servers = []
    try:
        package = companion_package() if any(spec.endswith("+companion") for spec in versions) else None
        for spec in versions:
            process, server = start(spec, fetch(spec.removesuffix("+companion")), media_root,
                                    package if spec.endswith("+companion") else None)
            processes.append(process)
            servers.append(server)
            print(f"Jellyfin {server['version']} at {server['url']}{' with the Companion' if server['companion'] else ''}", flush=True)
        (OUTPUT / "servers.json").write_text(json.dumps(servers, indent=2) + "\n")
        seed = OUTPUT / "upgrade-0-1-6"
        seed.mkdir(exist_ok=True)
        (seed / "config.json").write_text(json.dumps({"jellyfin_url": servers[0]["url"]}, indent=2))
        print("ready", flush=True)
        while True:
            time.sleep(3600)
    except KeyboardInterrupt:
        pass
    finally:
        for process in processes:
            process.terminate()
        for process in processes:
            process.wait(timeout=30)
    return 0


def interrupt(_signal: int, _frame: object) -> None:
    raise KeyboardInterrupt


if __name__ == "__main__":
    signal.signal(signal.SIGTERM, interrupt)
    sys.exit(main())
