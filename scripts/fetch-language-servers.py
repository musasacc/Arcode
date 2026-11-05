#!/usr/bin/env python3
"""Downloads the language servers bundled with Arcode for one platform.

Run through scripts/fetch-language-servers.sh; see that file and
docs/PACKAGING.md. Reads packaging/language-servers.toml, downloads every
archive it lists for the platform, checks each against its sha256 (a
mismatch stops the script and nothing is installed), installs the npm
servers from packaging/npm with its lockfile, builds gopls when Go is
available and lays everything out in <dest>/language-servers.

Needs Python 3.11 or newer (tomllib) and curl; npm servers need either the
downloaded Node.js (when the platform is the one this runs on) or npm on
PATH; gopls needs Go.
"""

from __future__ import annotations

import argparse
import gzip
import hashlib
import json
import os
import platform as host_platform
import re
import shutil
import stat
import subprocess
import sys
import tarfile
import tempfile
import zipfile
from pathlib import Path

try:
    import tomllib
except ModuleNotFoundError:  # Python < 3.11
    try:
        import tomli as tomllib  # type: ignore[no-redef]
    except ModuleNotFoundError:
        sys.exit("error: Python 3.11 or newer is needed (tomllib)")

ROOT = Path(__file__).resolve().parent.parent
MANIFEST = ROOT / "packaging" / "language-servers.toml"
NPM_DIR = ROOT / "packaging" / "npm"

PLATFORMS = {
    # name: (npm os, npm cpu, GOOS, GOARCH, executable suffix)
    "linux-x64": ("linux", "x64", "linux", "amd64", ""),
    "linux-arm64": ("linux", "arm64", "linux", "arm64", ""),
    "macos-arm64": ("darwin", "arm64", "darwin", "arm64", ""),
    "macos-x64": ("darwin", "x64", "darwin", "amd64", ""),
    "windows-x64": ("win32", "x64", "windows", "amd64", ".exe"),
}

STAMP = ".bundle-stamp"


def log(message: str) -> None:
    print(message, flush=True)


def fail(message: str) -> None:
    sys.exit(f"error: {message}")


def host_platform_name() -> str | None:
    system = host_platform.system()
    machine = host_platform.machine().lower()
    arch = {"x86_64": "x64", "amd64": "x64", "aarch64": "arm64", "arm64": "arm64"}.get(machine)
    if arch is None:
        return None
    if system == "Linux":
        return f"linux-{arch}"
    if system == "Darwin":
        return f"macos-{arch}"
    if system == "Windows" or system.startswith(("MINGW", "MSYS", "CYGWIN")):
        return f"windows-{arch}"
    return None


def sha256_of(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def download(url: str, sha256: str, cache: Path) -> Path:
    """The verified file for `url`, from the cache or downloaded into it."""
    if not re.fullmatch(r"[0-9a-f]{64}", sha256):
        fail(f"bad sha256 for {url}")
    name = url.rstrip("/").rsplit("/", 1)[-1] or "download"
    target = cache / "files" / sha256 / name
    if target.is_file():
        if sha256_of(target) == sha256:
            return target
        target.unlink()
    target.parent.mkdir(parents=True, exist_ok=True)
    partial = target.with_name(name + ".part")
    log(f"  downloading {url}")
    curl = shutil.which("curl")
    if curl is None:
        fail("curl is needed to download")
    result = subprocess.run(
        [curl, "-fsSL", "--retry", "3", "--connect-timeout", "30", "-o", str(partial), url],
        check=False,
    )
    if result.returncode != 0:
        partial.unlink(missing_ok=True)
        fail(f"download failed ({result.returncode}): {url}")
    actual = sha256_of(partial)
    if actual != sha256:
        partial.unlink(missing_ok=True)
        fail(f"checksum mismatch for {url}\n  expected {sha256}\n  got      {actual}")
    partial.replace(target)
    return target


def archive_kind(url: str) -> str:
    lower = url.lower()
    for suffix, kind in (
        (".tar.gz", "tar"),
        (".tgz", "tar"),
        (".tar.xz", "tar"),
        (".zip", "zip"),
        (".gz", "gz"),
    ):
        if lower.endswith(suffix):
            return kind
    return "raw"


def safe_join(root: Path, name: str) -> Path:
    path = (root / name).resolve()
    if path != root.resolve() and root.resolve() not in path.parents:
        fail(f"archive entry outside the target folder: {name}")
    return path


def extract(archive: Path, kind: str, into: Path) -> None:
    into.mkdir(parents=True, exist_ok=True)
    if kind == "tar":
        with tarfile.open(archive, "r:*") as tar:
            for member in tar.getmembers():
                safe_join(into, member.name)
            if hasattr(tarfile, "data_filter"):
                tar.extractall(into, filter="data")
            else:
                tar.extractall(into)  # entries were checked above
    elif kind == "zip":
        with zipfile.ZipFile(archive) as z:
            for info in z.infolist():
                target = safe_join(into, info.filename)
                if info.is_dir():
                    target.mkdir(parents=True, exist_ok=True)
                    continue
                target.parent.mkdir(parents=True, exist_ok=True)
                with z.open(info) as src, target.open("wb") as dst:
                    shutil.copyfileobj(src, dst)
                mode = (info.external_attr >> 16) & 0o777
                if mode:
                    os.chmod(target, mode | stat.S_IRUSR | stat.S_IWUSR)
    else:
        fail(f"cannot unpack {archive}")


def strip_root(folder: Path, strip: int) -> Path:
    for _ in range(strip):
        entries = list(folder.iterdir())
        if len(entries) != 1 or not entries[0].is_dir():
            fail(f"cannot strip a leading folder from {folder}")
        folder = entries[0]
    return folder


def make_executable(path: Path) -> None:
    path.chmod(path.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)


def copy_licenses(server: dict, unpacked: Path | None, cache: Path, out: Path) -> None:
    folder = out / "LICENSES" / server["name"]
    for entry in server.get("licenses", []):
        if "url" in entry:
            source = download(entry["url"], entry["sha256"], cache)
        else:
            if unpacked is None:
                fail(f"{server['name']}: no unpacked files for license {entry['path']}")
            source = unpacked / entry["path"]
            if not source.is_file():
                fail(f"{server['name']}: license {entry['path']} is missing")
        folder.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source, folder / source.name)


def install_archive(server: dict, plat: str, cache: Path, out: Path, work: Path) -> None:
    entry = server["platforms"][plat]
    archive = download(entry["url"], entry["sha256"], cache)
    kind = archive_kind(entry["url"])
    suffix = PLATFORMS[plat][4]
    install = server["install"]
    unpacked: Path | None = None
    if "file" in install:
        target = out / (install["file"] + suffix)
        target.parent.mkdir(parents=True, exist_ok=True)
        if kind == "raw":
            shutil.copyfile(archive, target)
        elif kind == "gz":
            with gzip.open(archive, "rb") as src, target.open("wb") as dst:
                shutil.copyfileobj(src, dst)
        else:
            unpacked = work / server["name"]
            extract(archive, kind, unpacked)
            member = entry.get("member")
            if member is None:
                files = [p for p in unpacked.rglob("*") if p.is_file()]
                if len(files) != 1:
                    fail(f"{server['name']}: name the executable with `member` for {plat}")
                source = files[0]
            else:
                source = safe_join(unpacked, member)
            if not source.is_file():
                fail(f"{server['name']}: {member} is not in the archive")
            shutil.copyfile(source, target)
        make_executable(target)
    else:
        unpacked = work / server["name"]
        extract(archive, kind, unpacked)
        root = strip_root(unpacked, int(install.get("strip", 0)))
        target = out / install["dir"]
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.move(str(root), str(target))
        unpacked = target
    copy_licenses(server, unpacked, cache, out)


def install_node(node: dict, plat: str, cache: Path, out: Path, work: Path) -> Path:
    """Installs the Node.js binary; returns the unpacked distribution."""
    entry = node["platforms"][plat]
    archive = download(entry["url"], entry["sha256"], cache)
    unpacked = work / "node"
    extract(archive, archive_kind(entry["url"]), unpacked)
    dist = strip_root(unpacked, 1)
    if plat.startswith("windows"):
        source, target = dist / "node.exe", out / "node" / "node.exe"
    else:
        source, target = dist / "bin" / "node", out / "node" / "bin" / "node"
    target.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(source, target)
    make_executable(target)
    copy_licenses({"name": "node", **node}, dist, cache, out)
    return dist


def npm_command(plat: str, node_dist: Path | None) -> list[str]:
    """How to run npm: the downloaded one when it can run here."""
    if node_dist is not None and host_platform_name() == plat:
        if plat.startswith("windows"):
            return [str(node_dist / "node.exe"), str(node_dist / "node_modules/npm/bin/npm-cli.js")]
        return [str(node_dist / "bin/node"), str(node_dist / "lib/node_modules/npm/bin/npm-cli.js")]
    npm = shutil.which("npm")
    if npm is None:
        fail("npm is needed to install the npm servers for another platform")
    return [npm]


def install_npm(
    servers: list[dict], plat: str, node_dist: Path | None, cache: Path, out: Path
) -> None:
    target = out / "lib" / "npm"
    target.mkdir(parents=True, exist_ok=True)
    for name in ("package.json", "package-lock.json"):
        shutil.copyfile(NPM_DIR / name, target / name)
    npm_os, npm_cpu = PLATFORMS[plat][0], PLATFORMS[plat][1]
    env = dict(os.environ)
    env.update(
        {
            "npm_config_cache": str(cache / "npm"),
            "npm_config_update_notifier": "false",
            "npm_config_fund": "false",
            "npm_config_audit": "false",
        }
    )
    command = npm_command(plat, node_dist) + [
        "ci",
        "--omit=dev",
        "--ignore-scripts",
        "--no-audit",
        "--no-fund",
        f"--os={npm_os}",
        f"--cpu={npm_cpu}",
    ]
    log("  npm ci (packaging/npm/package-lock.json)")
    if subprocess.run(command, cwd=target, env=env, check=False).returncode != 0:
        fail("npm ci failed")
    modules = target / "node_modules"
    for server in servers:
        for spec in server["packages"]:
            name, version = spec.rsplit("@", 1)
            installed = json.loads((modules / name / "package.json").read_text("utf-8"))["version"]
            if installed != version:
                fail(f"{name}: installed {installed}, the manifest pins {version}")
    lock = json.loads((target / "package-lock.json").read_text("utf-8"))
    for path in sorted(p for p in lock["packages"] if p):
        folder = target / path
        if not folder.is_dir():
            continue  # an optional package for another platform
        relative = path.removeprefix("node_modules/").replace("/node_modules/", "/")
        texts = [
            p
            for p in folder.iterdir()
            if p.is_file() and re.match(r"(?i)(licen[cs]e|copying|notice)", p.name)
        ]
        dest = out / "LICENSES" / "npm" / relative
        dest.mkdir(parents=True, exist_ok=True)
        for text in texts:
            shutil.copyfile(text, dest / text.name)
        if not texts:
            # Some packages publish no license file; record what they declare.
            meta = json.loads((folder / "package.json").read_text("utf-8"))
            repository = meta.get("repository")
            if isinstance(repository, dict):
                repository = repository.get("url")
            (dest / "LICENSE-DECLARED.txt").write_text(
                f"{meta.get('name')} {meta.get('version')}\n"
                f"License (from package.json): {meta.get('license', 'not declared')}\n"
                f"Source: {repository or meta.get('homepage') or 'https://www.npmjs.com/package/' + str(meta.get('name'))}\n"
                "The published package contains no license file.\n",
                "utf-8",
            )


def install_go(server: dict, plat: str, cache: Path, out: Path, work: Path) -> str | None:
    """Builds a Go server; returns why it was skipped, if it was."""
    go = shutil.which("go")
    if go is None:
        return "Go is not installed on the packaging machine"
    goos, goarch, suffix = PLATFORMS[plat][2], PLATFORMS[plat][3], PLATFORMS[plat][4]
    spec = f"{server['module']}@{server['version']}"
    env = dict(os.environ)
    # With GOBIN set, go install would put the binary there (outside the
    # bundle) and refuses cross-compiled installs altogether.
    env.pop("GOBIN", None)
    env.update(
        {
            "GOMODCACHE": str(cache / "gomod"),
            "GOPATH": str(work / "gopath"),
            "GOFLAGS": "-modcacherw",
            "CGO_ENABLED": "0",
        }
    )
    configured = subprocess.run(
        [go, "env", "GOBIN"], env=env, cwd=work, capture_output=True, text=True, check=False
    ).stdout.strip()
    if configured:
        fail(
            f"GOBIN is set in Go's configuration ({configured}); remove it with "
            "'go env -u GOBIN', or set ARCODE_LSP_SKIP_GO=1 to leave gopls out"
        )
    log(f"  go mod download {spec}")
    result = subprocess.run(
        [go, "mod", "download", "-json", spec],
        env=env,
        cwd=work,
        capture_output=True,
        text=True,
        check=False,
    )
    if result.returncode != 0:
        fail(f"go mod download {spec} failed:\n{result.stdout}{result.stderr}")
    info = json.loads(result.stdout)
    if info.get("Sum") != server["sum"]:
        fail(f"{spec}: checksum {info.get('Sum')} does not match the manifest ({server['sum']})")
    log(f"  go install {spec} ({goos}/{goarch})")
    build_env = dict(env, GOOS=goos, GOARCH=goarch)
    build = subprocess.run(
        [go, "install", "-trimpath", "-ldflags=-s -w", spec],
        env=build_env,
        cwd=work,
        check=False,
    )
    if build.returncode != 0:
        fail(f"go install {spec} failed")
    name = server["module"].rsplit("/", 1)[-1] + suffix
    bin_dir = work / "gopath" / "bin"
    built = bin_dir / name
    if not built.is_file():
        built = bin_dir / f"{goos}_{goarch}" / name
    if not built.is_file():
        fail(f"go install {spec} produced no {name}")
    command = server["commands"][0]["exec"] + suffix
    target = out / command
    target.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(built, target)
    make_executable(target)
    copy_licenses(server, Path(info["Dir"]), cache, out)
    return None


def third_party(
    manifest: dict, plat: str, included: list[dict], skipped: list[tuple[str, str]], out: Path
) -> str:
    lines = [
        "Third-party software bundled with Arcode",
        "=======================================",
        "",
        f"The language servers in Arcode's language-servers folder ({plat}; in an",
        "installation share/arcode/language-servers) are separate programs by",
        "their own authors, distributed unmodified under their own licenses. The",
        "license texts are in that folder's LICENSES/<name>/. Source code is",
        "available from each project's homepage.",
        "",
    ]
    node = manifest["node"]
    lines.append(f"- Node.js {node['version']} ({node['license']}): {node['homepage']}")
    lines.append("  The runtime of the npm servers below; not on PATH, used only by them.")
    for server in included:
        lines.append(
            f"- {server['name']} {server['version']} ({server['license']}): {server['homepage']}"
        )
    lock = json.loads((NPM_DIR / "package-lock.json").read_text("utf-8"))
    packages = sorted(
        (path.removeprefix("node_modules/").replace("/node_modules/", " > "), meta)
        for path, meta in lock["packages"].items()
        if path and not (meta.get("os") and PLATFORMS[plat][0] not in meta["os"])
    )
    lines += [
        "",
        "npm packages (installed from packaging/npm/package-lock.json; license",
        "texts in LICENSES/npm/<package>/):",
        "",
    ]
    lines += [
        f"  {name} {meta.get('version', '')} ({meta.get('license', 'see package')})"
        for name, meta in packages
    ]
    if skipped:
        lines += ["", "Not included in this build:", ""]
        lines += [f"- {name}: {why}" for name, why in skipped]
    for entry in manifest.get("not_bundled", []):
        lines.append(f"- {entry['name']} (not bundled): {entry['reason']}")
    return "\n".join(lines) + "\n"


def folder_size(path: Path) -> int:
    return sum(p.stat().st_size for p in path.rglob("*") if p.is_file() and not p.is_symlink())


def go_wanted() -> bool:
    return os.environ.get("ARCODE_LSP_SKIP_GO") != "1"


def stamp_for(plat: str, manifest_bytes: bytes) -> str:
    """Identifies a finished bundle: a run with another manifest or
    lockfile, or now with Go where gopls was skipped before, rebuilds it."""
    digest = hashlib.sha256(manifest_bytes)
    for name in ("package.json", "package-lock.json"):
        digest.update((NPM_DIR / name).read_bytes())
    go = "go" if go_wanted() and shutil.which("go") else "no-go"
    return f"{plat} {digest.hexdigest()} {go}\n"


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("platform", choices=sorted(PLATFORMS))
    parser.add_argument("dest", type=Path, help="writes <dest>/language-servers")
    parser.add_argument("--cache", type=Path, help="download cache (reused between runs)")
    parser.add_argument("--force", action="store_true", help="rebuild even when up to date")
    args = parser.parse_args()
    plat = args.platform

    manifest_bytes = MANIFEST.read_bytes()
    manifest = tomllib.loads(manifest_bytes.decode("utf-8"))
    out = args.dest.resolve() / "language-servers"
    stamp = stamp_for(plat, manifest_bytes)
    if not args.force and (out / STAMP).is_file() and (out / STAMP).read_text("utf-8") == stamp:
        log(f"{out} is up to date ({plat})")
        return

    default_cache = Path(os.environ.get("XDG_CACHE_HOME") or Path.home() / ".cache")
    cache = (
        args.cache
        or Path(os.environ.get("ARCODE_LSP_CACHE") or default_cache / "arcode-language-servers")
    ).resolve()
    cache.mkdir(parents=True, exist_ok=True)
    out.parent.mkdir(parents=True, exist_ok=True)
    staging = Path(tempfile.mkdtemp(prefix=".language-servers-", dir=out.parent))
    work = Path(tempfile.mkdtemp(prefix="arcode-lsp-work-"))
    try:
        log(f"fetching language servers for {plat} into {out} (cache {cache})")
        log("node")
        node_dist = install_node(manifest["node"], plat, cache, staging, work)
        included: list[dict] = []
        skipped: list[tuple[str, str]] = []
        npm_servers = []
        for server in manifest["server"]:
            name = server["name"]
            if plat in server.get("unsupported", []):
                skipped.append((name, server.get("note", "no build for this platform")))
                log(f"{name}: skipped ({server.get('note', 'no build for this platform')})")
                continue
            source = server["source"]
            if source == "archive":
                log(name)
                install_archive(server, plat, cache, staging, work)
            elif source == "npm":
                npm_servers.append(server)
                continue
            elif source == "go":
                log(name)
                why = None if go_wanted() else "skipped on request"
                why = why or install_go(server, plat, cache, staging, work)
                if why is not None:
                    note = f"{why}; install it with: go install {server['module']}@{server['version']}"
                    skipped.append((name, note))
                    log(f"{name}: skipped ({why})")
                    continue
            else:
                fail(f"{name}: unknown source {source!r}")
            included.append(server)
        if npm_servers:
            log(", ".join(s["name"] for s in npm_servers))
            install_npm(npm_servers, plat, node_dist, cache, staging)
            included += npm_servers
        shutil.copyfile(MANIFEST, staging / "manifest.toml")
        (staging / "THIRD-PARTY.txt").write_text(
            third_party(manifest, plat, included, skipped, staging), "utf-8"
        )
        if skipped:
            (staging / "SKIPPED.txt").write_text(
                "".join(f"{name}: {why}\n" for name, why in skipped), "utf-8"
            )
        (staging / STAMP).write_text(stamp, "utf-8")
        old = None
        if out.exists():
            old = out.with_name(out.name + ".old")
            shutil.rmtree(old, ignore_errors=True)
            out.rename(old)
        staging.rename(out)
        if old is not None:
            shutil.rmtree(old, ignore_errors=True)
    finally:
        shutil.rmtree(work, ignore_errors=True)
        shutil.rmtree(staging, ignore_errors=True)
    size = folder_size(out)
    log(f"done: {len(included)} servers, {size / 1_000_000:.0f} MB in {out}")
    for name, why in skipped:
        log(f"  not included: {name} ({why})")


if __name__ == "__main__":
    main()
