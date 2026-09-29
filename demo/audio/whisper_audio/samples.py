"""The sample cache: download, verify and read the sample libraries listed in `samples.toml`.

Every library is one archive, pinned by URL and SHA-256. `fetch` downloads a
missing archive into the cache and verifies it; an archive already in the cache
is only verified, so a primed cache needs no network. `read_members` reads files
straight out of a verified archive in one pass, so nothing is unpacked on disk.

A missing download or a checksum mismatch raises `SampleCacheError` naming the
library. A bad archive already in the cache is never replaced silently: delete
it and run the fetch again.

Usage: python -m whisper_audio.samples   (the build's "Fetch samples" step)
"""

from __future__ import annotations

import hashlib
import os
import shutil
import sys
import tarfile
import tomllib
import urllib.request
from collections.abc import Callable
from dataclasses import dataclass
from pathlib import Path

AUDIO_PROJECT = Path(__file__).resolve().parents[1]
MANIFEST = AUDIO_PROJECT / "samples.toml"
CACHE_DIR = AUDIO_PROJECT.parent / ".cache" / "samples"


class SampleCacheError(RuntimeError):
    pass


@dataclass(frozen=True)
class Library:
    id: str
    name: str
    role: str
    version: str
    url: str
    sha256: str
    sfz: str  # path of the SFZ instrument inside the archive

    @property
    def archive_name(self) -> str:
        return self.url.rsplit("/", 1)[-1]

    def archive(self, cache_dir: Path) -> Path:
        return cache_dir / self.id / self.archive_name

    def error(self, message: str) -> SampleCacheError:
        return SampleCacheError(f"sample library '{self.id}' ({self.name}, {self.version}): {message}")


def load_manifest(path: Path = MANIFEST) -> list[Library]:
    data = tomllib.loads(path.read_text(encoding="utf-8"))
    fields = ("name", "role", "version", "url", "sha256", "sfz")
    return [Library(id=key, **{f: entry[f] for f in fields}) for key, entry in data["libraries"].items()]


def sha256_of(path: Path) -> str:
    digest = hashlib.sha256()
    with open(path, "rb") as f:
        for block in iter(lambda: f.read(1 << 20), b""):
            digest.update(block)
    return digest.hexdigest()


def verified_archive(library: Library, cache_dir: Path = CACHE_DIR) -> Path:
    """The library's cached archive, checked against the manifest. Never touches the network."""
    path = library.archive(cache_dir)
    if not path.is_file():
        raise library.error(f"not in the sample cache ({path}); run `pnpm run build` to download it")
    actual = sha256_of(path)
    if actual != library.sha256:
        raise library.error(
            f"cached archive {path} does not match the manifest "
            f"(SHA-256 {actual}, expected {library.sha256}); delete it and run the build again to re-download"
        )
    return path


def download(library: Library, cache_dir: Path = CACHE_DIR) -> Path:
    """Download the library's archive into the cache, keeping it only if its SHA-256 matches."""
    path = library.archive(cache_dir)
    path.parent.mkdir(parents=True, exist_ok=True)
    partial = path.with_name(path.name + ".part")
    request = urllib.request.Request(library.url, headers={"User-Agent": "whisper-demo-build"})
    try:
        with urllib.request.urlopen(request, timeout=60) as response, open(partial, "wb") as out:
            shutil.copyfileobj(response, out, 1 << 20)
    except OSError as err:  # urllib's URLError is an OSError
        partial.unlink(missing_ok=True)
        raise library.error(f"download from {library.url} failed: {err}") from err
    actual = sha256_of(partial)
    if actual != library.sha256:
        partial.unlink()
        raise library.error(
            f"download from {library.url} does not match the manifest (SHA-256 {actual}, expected {library.sha256})"
        )
    os.replace(partial, path)
    return path


def fetch(libraries: list[Library], cache_dir: Path = CACHE_DIR, log=print) -> None:
    """Make sure every library's archive is in the cache and verified, downloading only missing ones."""
    for library in libraries:
        if library.archive(cache_dir).is_file():
            verified_archive(library, cache_dir)
            log(f"  {library.id}: cached, verified")
        else:
            log(f"  {library.id}: downloading {library.url}")
            download(library, cache_dir)
            log(f"  {library.id}: downloaded, verified")


def read_members(library: Library, wanted: Callable[[str], bool], cache_dir: Path = CACHE_DIR) -> dict[str, bytes]:
    """Every file in the library's verified archive (.7z or a tarball) whose archive path `wanted` accepts,
    keyed by that path. The archive is verified and decompressed once."""
    path = verified_archive(library, cache_dir)
    found: dict[str, bytes] = {}
    if path.name.endswith(".7z"):
        import py7zr
        from py7zr.io import BytesIOFactory

        factory = BytesIOFactory(limit=1 << 30)
        with py7zr.SevenZipFile(path) as archive:
            targets = [info.filename for info in archive.list() if not info.is_directory and wanted(info.filename)]
            archive.extract(targets=targets, factory=factory)
        for name in targets:
            product = factory.get(name)
            product.seek(0)
            found[name] = product.read()
    else:
        with tarfile.open(path) as archive:
            for member in archive:
                if member.isfile() and wanted(member.name):
                    found[member.name] = archive.extractfile(member).read()
    return found


def main() -> None:
    try:
        fetch(load_manifest())
    except SampleCacheError as err:
        sys.exit(f"error: {err}")
    print(f"sample cache: {CACHE_DIR}")


if __name__ == "__main__":
    main()
