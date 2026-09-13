"""Bundle the private playback libraries and their exact Debian/Ubuntu sources."""

import os
from pathlib import Path
import re
import shutil
import subprocess
import sys


# These interfaces must come from the host together with its drivers, audio
# plugins, desktop, and libc. Do not traverse or copy their private dependencies.
HOST_LIBRARIES = re.compile(
    r"^(?:ld-linux[^/]*|lib(?:c|m|pthread|dl|rt|resolv|util|gcc_s|stdc\+\+)\.so.*"
    r"|lib(?:EGL|GL|GLX|OpenGL|GLdispatch|glapi|drm[^/]*|va[^/]*|vdpau|vulkan|cuda|nvcuvid|nvidia-[^/]*)\.so.*"
    r"|lib(?:X[^/]*|xcb[^/]*|wayland[^/]*)\.so.*"
    r"|lib(?:asound|pulse|pulse-simple|pulsecommon-[^/]*)\.so.*)$"
)


def run(*args, **kwargs):
    return subprocess.check_output(args, text=True, **kwargs).strip()


def dependencies(library, prefix):
    env = {**os.environ, "LC_ALL": "C", "LD_LIBRARY_PATH": str(prefix / "lib")}
    resolved = {}
    for line in run("ldd", str(library), env=env).splitlines():
        match = re.match(r"\s*(\S+) => (/\S+) \(", line)
        if match:
            resolved[match[1]] = Path(match[2])
        else:
            # The ELF loader can itself be a DT_NEEDED entry (e.g. GnuTLS).
            # ldd prints its absolute path without the usual "name =>" prefix.
            match = re.match(r"\s*(/\S+) \(", line)
            if match:
                path = Path(match[1])
                resolved[path.name] = path
    for soname in run("patchelf", "--print-needed", str(library)).splitlines():
        if soname not in resolved:
            raise RuntimeError(f"Unresolved dependency {soname} required by {library}")
        yield soname, resolved[soname]


def package_for(library):
    # dpkg may record /lib or /usr/lib, depending on the release's usr-merge.
    candidates = [str(library), str(library.resolve())]
    if str(library).startswith("/usr/lib/"):
        candidates.append(str(library)[4:])
    for candidate in candidates:
        result = subprocess.run(
            ["dpkg-query", "-S", candidate], text=True, capture_output=True, check=False
        )
        if result.returncode == 0:
            return result.stdout.split(": /", 1)[0]
    raise RuntimeError(f"Cannot identify package/source for bundled library: {library}")


def bundle(prefix, output, source_dir, build_packages=()):
    lib_dir = output / "lib"
    licenses = output / "licenses"
    for directory in (lib_dir, licenses):
        if directory.exists():
            shutil.rmtree(directory)
        directory.mkdir(parents=True)
    source_dir.mkdir(parents=True, exist_ok=True)

    pending = [("libmpv.so.2", prefix / "lib/libmpv.so.2")]
    seen = set()
    packages = set(build_packages)
    host = set()
    while pending:
        soname, library = pending.pop()
        if soname in seen:
            continue
        seen.add(soname)
        if HOST_LIBRARIES.fullmatch(soname):
            host.add(soname)
            continue
        if not library.resolve().is_relative_to(prefix):
            packages.add(package_for(library))
        pending.extend(dependencies(library, prefix))
        target = lib_dir / soname
        shutil.copy2(library.resolve(), target)
        subprocess.run(["strip", "--strip-unneeded", str(target)], check=True)
        subprocess.run(["patchelf", "--set-rpath", "$ORIGIN", str(target)], check=True)

    records = []
    fetched = set()
    for package in sorted(packages):
        metadata = run(
            "dpkg-query", "-W", "-f=${binary:Package}\t${Version}\t${source:Package}\t${source:Version}",
            package,
        )
        records.append(metadata)
        binary, _, source, version = metadata.split("\t")
        notice = Path("/usr/share/doc") / binary.split(":")[0] / "copyright"
        shutil.copy2(notice, licenses / f"{binary.replace(':', '-')}.copyright")
        if (source, version) not in fetched:
            subprocess.run(
                ["apt-get", "source", "--download-only", "--only-source", f"{source}={version}"],
                cwd=source_dir, check=True,
            )
            fetched.add((source, version))
    # Debian copyright files can reference common license texts by absolute path.
    shutil.copytree("/usr/share/common-licenses", licenses / "common-licenses")
    (output / "SYSTEM-PACKAGES.txt").write_text("\n".join(records) + "\n")
    (output / "HOST-LIBRARIES.txt").write_text("\n".join(sorted(host)) + "\n")


if __name__ == "__main__":
    bundle(*(Path(arg).resolve() for arg in sys.argv[1:4]), build_packages=sys.argv[4:])
