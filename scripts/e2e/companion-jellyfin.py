#!/usr/bin/env python3
"""End-to-end check that the packaged Companion loads on real Jellyfin servers.

Builds and packages the plugin exactly like a release (dotnet publish, then
plugin/package.py), downloads the official portable Linux server for each
requested Jellyfin minor version, installs the package with its meta.json into
a disposable data directory, and drives the server over HTTP on loopback:

  - complete the startup wizard and sign in as the created administrator
  - the server log must report the Companion as loaded, and /Plugins must
    list it with the packaged version
  - /MediaFlick/info must reject anonymous calls and answer signed-in calls

Run it with no arguments to test the plugin's minimum Jellyfin version (the
Jellyfin.Controller package version) and the newest minor release of the same
major version, or name minor versions explicitly:

  python3 scripts/e2e/companion-jellyfin.py [12.0 12.2 ...]

The output directory (default build/e2e/companion-jellyfin) receives
report.json plus each server's console and Jellyfin logs. Downloaded servers
are cached in build/e2e/jellyfin-servers. The script exits non-zero when an
invariant fails. Linux x64 and arm64 only.
"""

from __future__ import annotations

import argparse
import importlib.util
import json
import os
import platform
import re
import shutil
import signal
import socket
import subprocess
import sys
import tarfile
import tempfile
import time
import urllib.error
import urllib.request
import zipfile
from dataclasses import dataclass
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
PLUGIN = ROOT / "plugin"
GUID = "11d8f2bb-2b9d-4ce1-8c33-5a0f809dfd2f"
RELEASES = "https://repo.jellyfin.org/files/server/linux/stable/"
CLIENT = 'MediaBrowser Client="MediaFlick E2E", Device="e2e", DeviceId="mediaflick-companion-e2e", Version="1.0.0"'
ADMIN_USER = "e2e-admin"
ADMIN_PASSWORD = "e2e-password"
START_TIMEOUT_SECONDS = 180


@dataclass
class Response:
    status: int
    body: str

    def json(self) -> object:
        return json.loads(self.body)


def load_package_module():
    spec = importlib.util.spec_from_file_location("companion_package", PLUGIN / "package.py")
    if spec is None or spec.loader is None:
        raise SystemExit("cannot load plugin/package.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def server_arch() -> str:
    machine = platform.machine().lower()
    arch = {"x86_64": "amd64", "amd64": "amd64", "aarch64": "arm64", "arm64": "arm64"}.get(machine)
    if sys.platform != "linux" or arch is None:
        raise SystemExit(f"portable Jellyfin servers are only fetched for Linux x64/arm64, not {sys.platform}/{machine}")
    return arch


def newest_minor(major: str) -> str:
    with urllib.request.urlopen(RELEASES, timeout=30) as listing:
        index = listing.read().decode("utf-8")
    minors = {int(minor) for minor in re.findall(rf'href="v{major}\.(\d+)/"', index)}
    if not minors:
        raise SystemExit(f"no Jellyfin {major}.x release listed at {RELEASES}")
    return f"{major}.{max(minors)}"


def fetch_server(version: str, cache: Path, arch: str) -> Path:
    server = cache / f"{version}-{arch}" / "jellyfin"
    if (server / "jellyfin").is_file():
        return server
    name = f"jellyfin_{version}-{arch}.tar.xz"
    url = f"{RELEASES}v{version}/{arch}/{name}"
    download = cache / f"{version}-{arch}"
    if download.exists():
        shutil.rmtree(download)
    download.mkdir(parents=True)
    archive = download / name
    print(f"downloading {url}", flush=True)
    with urllib.request.urlopen(url, timeout=300) as source, archive.open("wb") as target:
        shutil.copyfileobj(source, target)
    with tarfile.open(archive) as tar:
        tar.extractall(download, filter="data")
    archive.unlink()
    if not (server / "jellyfin").is_file():
        raise SystemExit(f"{name} did not contain jellyfin/jellyfin")
    return server


def build_package(work: Path) -> tuple[Path, dict[str, str]]:
    props = (PLUGIN / "Directory.Build.props").read_text(encoding="utf-8")
    version_match = re.search(r"<Version>([^<]+)</Version>", props)
    if version_match is None:
        raise SystemExit("plugin/Directory.Build.props has no <Version>")
    publish = work / "publish"
    subprocess.run(
        ["dotnet", "publish", str(PLUGIN / "src" / "Jellyfin.Plugin.MediaFlick" / "Jellyfin.Plugin.MediaFlick.csproj"),
         "--configuration", "Release", "--output", str(publish)],
        check=True,
    )
    package = work / "package"
    subprocess.run(
        [sys.executable, str(PLUGIN / "package.py"), "--version", version_match.group(1),
         "--publish-dir", str(publish), "--output-dir", str(package),
         "--source-url", "https://example.invalid/mediaflick-companion.zip", "--changelog", "E2E build"],
        check=True,
    )
    archive = next(package.glob("mediaflick-companion_*.zip"))
    with zipfile.ZipFile(archive) as zipped:
        meta = json.loads(zipped.read("meta.json"))
    return archive, meta


def free_port() -> int:
    with socket.socket() as probe:
        probe.bind(("127.0.0.1", 0))
        return int(probe.getsockname()[1])


def request(base: str, method: str, path: str, *, token: str | None = None, body: object | None = None) -> Response:
    authorization = CLIENT if token is None else f'{CLIENT}, Token="{token}"'
    data = None if body is None else json.dumps(body).encode("utf-8")
    headers = {"Authorization": authorization, "Accept": "application/json"}
    if data is not None:
        headers["Content-Type"] = "application/json"
    call = urllib.request.Request(base + path, data=data, method=method, headers=headers)
    try:
        with urllib.request.urlopen(call, timeout=60) as reply:
            return Response(reply.status, reply.read().decode("utf-8"))
    except urllib.error.HTTPError as error:
        return Response(error.code, error.read().decode("utf-8", "replace"))


def expect_ok(response: Response, step: str) -> Response:
    if not 200 <= response.status < 300:
        raise RuntimeError(f"{step} returned HTTP {response.status}: {response.body[:500]}")
    return response


def wait_for_server(base: str, process: subprocess.Popen[bytes]) -> dict[str, object]:
    deadline = time.monotonic() + START_TIMEOUT_SECONDS
    while time.monotonic() < deadline:
        if process.poll() is not None:
            raise RuntimeError(f"Jellyfin exited with code {process.returncode} during startup")
        try:
            # While migrations run, a temporary setup server answers this route
            # without a version and returns 503 for everything else.
            response = request(base, "GET", "/System/Info/Public")
            if response.status == 200:
                info = response.json()
                if isinstance(info, dict) and info.get("Version"):
                    return info
        except (urllib.error.URLError, ConnectionError, TimeoutError):
            pass
        time.sleep(1)
    raise RuntimeError(f"Jellyfin did not answer within {START_TIMEOUT_SECONDS}s")


def sign_in(base: str) -> str:
    expect_ok(request(base, "POST", "/Startup/Configuration",
                      body={"UICulture": "en-US", "MetadataCountryCode": "US", "PreferredMetadataLanguage": "en"}),
              "startup configuration")
    expect_ok(request(base, "GET", "/Startup/User"), "startup user lookup")
    expect_ok(request(base, "POST", "/Startup/User", body={"Name": ADMIN_USER, "Password": ADMIN_PASSWORD}),
              "startup user")
    expect_ok(request(base, "POST", "/Startup/Complete"), "startup completion")
    session = expect_ok(request(base, "POST", "/Users/AuthenticateByName",
                                body={"Username": ADMIN_USER, "Pw": ADMIN_PASSWORD}), "sign-in").json()
    if not isinstance(session, dict) or not isinstance(session.get("AccessToken"), str):
        raise RuntimeError("sign-in returned no access token")
    return session["AccessToken"]


def run_server(version: str, server: Path, archive: Path, meta: dict[str, str], output: Path, scratch_base: Path) -> dict[str, object]:
    result: dict[str, object] = {"requested": version, "passed": False}
    output.mkdir(parents=True, exist_ok=True)
    # Jellyfin refuses to start with less than 2 GiB free, which a tmpfs /tmp
    # often lacks, so the disposable profile lives next to the server cache.
    with tempfile.TemporaryDirectory(prefix=f"jellyfin-{version}-", dir=scratch_base) as scratch:
        root = Path(scratch)
        data, config, cache, logs, temp = (root / name for name in ("data", "config", "cache", "log", "tmp"))
        temp.mkdir()
        plugin_dir = data / "plugins" / f"{meta['name']}_{meta['version']}"
        plugin_dir.mkdir(parents=True)
        with zipfile.ZipFile(archive) as zipped:
            zipped.extractall(plugin_dir)
        config.mkdir()
        port = free_port()
        (config / "network.xml").write_text(
            '<?xml version="1.0" encoding="utf-8"?>\n<NetworkConfiguration>\n'
            f"  <InternalHttpPort>{port}</InternalHttpPort>\n"
            "  <EnableIPv6>false</EnableIPv6>\n"
            "  <LocalNetworkAddresses><string>127.0.0.1</string></LocalNetworkAddresses>\n"
            "  <AutoDiscovery>false</AutoDiscovery>\n"
            "</NetworkConfiguration>\n",
            encoding="utf-8",
        )
        command = [str(server / "jellyfin"), "--datadir", str(data), "--configdir", str(config),
                   "--cachedir", str(cache), "--logdir", str(logs), "--nowebclient"]
        ffmpeg = shutil.which("ffmpeg")
        if ffmpeg:
            command += ["--ffmpeg", ffmpeg]
        base = f"http://127.0.0.1:{port}"
        console = (output / "console.log").open("wb")
        environment = {**os.environ, "TMPDIR": str(temp)}
        process = subprocess.Popen(command, stdout=console, stderr=subprocess.STDOUT, env=environment, start_new_session=True)
        try:
            public = wait_for_server(base, process)
            result["serverVersion"] = public.get("Version")
            token = sign_in(base)

            plugins = expect_ok(request(base, "GET", "/Plugins", token=token), "plugin list").json()
            entry = next((p for p in plugins if isinstance(p, dict) and p.get("Id", "").replace("-", "") == GUID.replace("-", "")), None) \
                if isinstance(plugins, list) else None
            result["plugin"] = entry
            anonymous = request(base, "GET", "/MediaFlick/info")
            signed_in = request(base, "GET", "/MediaFlick/info", token=token)
            result["anonymousInfoStatus"] = anonymous.status
            result["infoStatus"] = signed_in.status
            info = signed_in.json() if signed_in.status == 200 else signed_in.body[:500]
            result["info"] = info

            # Jellyfin 12.0 lists a plugin it skipped for a too-new targetAbi as
            # Active, so only its own load message and live routes prove loading.
            loaded = f"Loaded plugin: {meta['name']} {meta['version']}"
            checks = {
                "pluginLoaded": loaded in (output / "console.log").read_text(encoding="utf-8", errors="replace"),
                "pluginVersionMatchesPackage": isinstance(entry, dict)
                and str(entry.get("Version", "")).startswith(meta["version"]),
                "anonymousInfoRejected": anonymous.status == 401,
                "infoAnswers": isinstance(info, dict) and info.get("apiVersion") == 1
                and isinstance(info.get("capabilities"), list),
            }
            result["checks"] = checks
            result["passed"] = all(checks.values())
        except (RuntimeError, ValueError) as error:
            result["error"] = str(error)
        finally:
            if process.poll() is None:
                process.send_signal(signal.SIGTERM)
                try:
                    process.wait(timeout=30)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait()
            console.close()
            if logs.is_dir():
                shutil.copytree(logs, output / "logs", dirs_exist_ok=True)
    return result


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("versions", nargs="*", help="Jellyfin minor versions such as 12.0 (default: floor and newest)")
    parser.add_argument("--output-dir", type=Path, default=ROOT / "build" / "e2e" / "companion-jellyfin")
    parser.add_argument("--cache-dir", type=Path, default=ROOT / "build" / "e2e" / "jellyfin-servers")
    args = parser.parse_args()
    arch = server_arch()

    target_abi = load_package_module().target_abi()
    major, minor = target_abi.split(".")[:2]
    versions = args.versions or list(dict.fromkeys([f"{major}.{minor}", newest_minor(major)]))
    for version in versions:
        if not re.fullmatch(r"\d+\.\d+", version):
            parser.error(f"expected a minor version such as 12.0, got {version!r}")

    if args.output_dir.exists():
        shutil.rmtree(args.output_dir)
    args.output_dir.mkdir(parents=True)
    args.cache_dir.mkdir(parents=True, exist_ok=True)

    with tempfile.TemporaryDirectory(prefix="companion-package-") as work:
        archive, meta = build_package(Path(work))
        results = []
        for version in versions:
            server = fetch_server(version, args.cache_dir, arch)
            print(f"testing Jellyfin {version}", flush=True)
            results.append(run_server(version, server, archive, meta, args.output_dir / version, args.cache_dir))

    report = {
        "package": {"version": meta["version"], "targetAbi": meta["targetAbi"]},
        "servers": results,
        "passed": all(result["passed"] for result in results),
    }
    (args.output_dir / "report.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    for result in results:
        state = "PASS" if result["passed"] else "FAIL"
        print(f"{state} Jellyfin {result.get('serverVersion', result['requested'])}: {result.get('checks', result.get('error'))}")
    print(f"report: {args.output_dir / 'report.json'}")
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
