#!/usr/bin/env python3
"""Stage the pinned, relocatable Python runtime used by the macOS arm64 app."""

from __future__ import annotations

import hashlib
import json
import os
import shutil
import subprocess
import sys
import tarfile
import tempfile
import urllib.request
from pathlib import Path
from typing import BinaryIO, TypedDict, cast

PACKAGING_DIR = Path(__file__).resolve().parents[1]
PROJECT_ROOT = PACKAGING_DIR.parents[1]
LOCK_PATH = PACKAGING_DIR / "runtime.lock.json"
REQUIREMENTS_PATH = PACKAGING_DIR / "requirements.txt"
CACHE_DIR = PACKAGING_DIR / ".cache"
BUILD_DIR = PACKAGING_DIR / "build"
PYTHON_STDLIB = Path("lib/python3.12")
SITE_PACKAGES = PYTHON_STDLIB / "site-packages"
DYLIB_NAME = "libpython3.12.dylib"


class RuntimeLock(TypedDict):
    python_version: str
    release: str
    archive: str
    url: str
    sha256: str
    python_tag: str
    platform_tag: str


class RuntimeManifest(TypedDict):
    python: str
    release: str
    archive_sha256: str
    requirements_sha256: str
    model_sha256: str


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def directory_sha256(root: Path) -> str:
    digest = hashlib.sha256()
    for path in sorted(item for item in root.rglob("*") if item.is_file()):
        if path.name == ".DS_Store" or "__pycache__" in path.parts:
            continue
        digest.update(path.relative_to(root).as_posix().encode("utf-8"))
        with path.open("rb") as stream:
            for chunk in iter(lambda: stream.read(1024 * 1024), b""):
                digest.update(chunk)
    return digest.hexdigest()


def download(url: str, destination: Path) -> None:
    destination.parent.mkdir(parents=True, exist_ok=True)
    temporary = destination.with_suffix(destination.suffix + ".download")
    temporary.unlink(missing_ok=True)
    print(f"Downloading {url}")
    response = cast(BinaryIO, urllib.request.urlopen(url))
    with response, temporary.open("wb") as output:
        shutil.copyfileobj(response, output)
    _ = temporary.replace(destination)


def checked_archive(lock: RuntimeLock) -> Path:
    archive = CACHE_DIR / lock["archive"]
    if not archive.exists():
        download(lock["url"], archive)

    actual = sha256(archive)
    expected = lock["sha256"]
    if actual != expected:
        archive.unlink(missing_ok=True)
        raise RuntimeError(
            f"Python archive checksum mismatch: expected {expected}, got {actual}"
        )
    return archive


def extract_runtime(archive: Path, destination: Path) -> Path:
    with tarfile.open(archive, "r:gz") as bundle:
        bundle.extractall(destination, filter="data")
    runtime = destination / "python"
    if not (runtime / "bin/python3.12").is_file():
        raise RuntimeError("standalone archive does not contain python/bin/python3.12")
    if not (runtime / "lib" / DYLIB_NAME).is_file():
        raise RuntimeError(f"standalone archive does not contain python/lib/{DYLIB_NAME}")
    return runtime


def remove_caches(root: Path) -> None:
    for path in sorted(root.rglob("*"), reverse=True):
        if path.name == ".DS_Store" or path.suffix in {".pyc", ".pyo"}:
            path.unlink(missing_ok=True)
        elif path.is_dir() and path.name == "__pycache__":
            shutil.rmtree(path)


def run(command: list[str], *, env: dict[str, str] | None = None) -> None:
    print("+", " ".join(command))
    _ = subprocess.run(command, cwd=PROJECT_ROOT, env=env, check=True)


def verify_staged_runtime(interpreter: Path, python_home: Path) -> None:
    site_packages = python_home / SITE_PACKAGES
    environment = os.environ.copy()
    environment.update(
        {
            "PYTHONHOME": str(python_home),
            "PYTHONPATH": str(site_packages),
            "PYTHONNOUSERSITE": "1",
            "PYTHONSAFEPATH": "1",
            "PYTHONDONTWRITEBYTECODE": "1",
            "HF_HUB_OFFLINE": "1",
            "TRANSFORMERS_OFFLINE": "1",
            "TOKENIZERS_PARALLELISM": "false",
        }
    )
    for name in ("PYTHONUSERBASE", "PYTHONSTARTUP", "VIRTUAL_ENV", "CONDA_PREFIX"):
        _ = environment.pop(name, None)

    verification = """
import pathlib
import sys
import torch
import transformers
root = pathlib.Path(sys.argv[1]).resolve()
site = (root / 'lib/python3.12/site-packages').resolve()
for name, module in [('torch', torch), ('transformers', transformers)]:
    origin = pathlib.Path(module.__file__).resolve()
    if not origin.is_relative_to(site):
        raise SystemExit(f'{name} escaped staged site-packages: {origin}')
print(sys.version)
print(torch.__version__)
print(transformers.__version__)
print(torch.__file__)
"""
    run([str(interpreter), "-c", verification, str(python_home)], env=environment)


def write_pyo3_config(destination: Path, lock: RuntimeLock) -> None:
    config = "\n".join(
        [
            "implementation=CPython",
            "version=3.12",
            "shared=true",
            "abi3=false",
            "lib_name=python3.12",
            f"lib_dir={destination}",
            "pointer_width=64",
            "build_flags=",
            "suppress_build_script_link_lines=false",
            f"# runtime={lock['python_version']}+{lock['release']}",
            "",
        ]
    )
    _ = (BUILD_DIR / "pyo3-config.txt").write_text(config, encoding="utf-8")


def main() -> int:
    if sys.platform != "darwin" or os.uname().machine != "arm64":
        raise RuntimeError("the macOS runtime must be prepared on an Apple Silicon Mac")

    raw_lock_value = cast(
        object, json.loads(LOCK_PATH.read_text(encoding="utf-8"))
    )
    if not isinstance(raw_lock_value, dict):
        raise TypeError(f"runtime lock must be a JSON object: {LOCK_PATH}")
    raw_lock = cast(dict[str, object], raw_lock_value)
    try:
        lock = RuntimeLock(
            python_version=str(raw_lock["python_version"]),
            release=str(raw_lock["release"]),
            archive=str(raw_lock["archive"]),
            url=str(raw_lock["url"]),
            sha256=str(raw_lock["sha256"]),
            python_tag=str(raw_lock["python_tag"]),
            platform_tag=str(raw_lock["platform_tag"]),
        )
    except KeyError as error:
        raise ValueError(f"runtime lock is missing {error.args[0]!r}") from error
    archive = checked_archive(lock)
    _ = CACHE_DIR.mkdir(parents=True, exist_ok=True)
    model_source = PROJECT_ROOT / "local-model"
    if not (model_source / "config.json").is_file():
        raise RuntimeError(f"local model is missing from {model_source}")
    manifest = RuntimeManifest(
        python=lock["python_version"],
        release=lock["release"],
        archive_sha256=lock["sha256"],
        requirements_sha256=sha256(REQUIREMENTS_PATH),
        model_sha256=directory_sha256(model_source),
    )
    manifest_path = BUILD_DIR / "runtime-manifest.json"
    required_staged_files = [
        BUILD_DIR / DYLIB_NAME,
        BUILD_DIR / "python/bin/python3.12",
        BUILD_DIR / "python/lib/python3.12/site-packages/torch/__init__.py",
        BUILD_DIR / "python/lib/python3.12/site-packages/transformers/__init__.py",
        BUILD_DIR / "local-model/config.json",
    ]
    remove_caches(BUILD_DIR)
    if manifest_path.is_file() and all(path.is_file() for path in required_staged_files):
        try:
            existing_manifest = cast(
                object, json.loads(manifest_path.read_text(encoding="utf-8"))
            )
        except json.JSONDecodeError:
            existing_manifest = None
        if existing_manifest == manifest:
            write_pyo3_config(BUILD_DIR, lock)
            print(f"Reusing bundled Python runtime at {BUILD_DIR}")
            return 0

    with tempfile.TemporaryDirectory(prefix="riimail-python-") as temporary:
        runtime = extract_runtime(archive, Path(temporary))
        staged = Path(temporary) / "staged"
        python_home = staged / "python"
        (python_home / PYTHON_STDLIB.parent).mkdir(parents=True, exist_ok=True)

        _ = shutil.copytree(runtime / PYTHON_STDLIB, python_home / PYTHON_STDLIB)
        _ = (python_home / "bin").mkdir(parents=True, exist_ok=True)
        _ = shutil.copy2(runtime / "bin/python3.12", python_home / "bin/python3.12")
        site_packages = python_home / SITE_PACKAGES
        shutil.rmtree(site_packages, ignore_errors=True)
        site_packages.mkdir(parents=True, exist_ok=True)

        run(
            [
                str(runtime / "bin/python3.12"),
                "-m",
                "pip",
                "install",
                "--requirement",
                str(REQUIREMENTS_PATH),
                "--target",
                str(site_packages),
                "--only-binary=:all:",
                "--no-compile",
                "--cache-dir",
                str(CACHE_DIR / "pip"),
                "--disable-pip-version-check",
            ]
        )

        remove_caches(python_home)
        libomp = site_packages / "torch/lib/libomp.dylib"
        if not libomp.is_file():
            raise RuntimeError(f"torch wheel does not contain {libomp}")
        run(["install_name_tool", "-id", "@rpath/libomp.dylib", str(libomp)])
        run(["codesign", "--force", "--sign", "-", "--timestamp=none", str(libomp)])
        verify_staged_runtime(python_home / "bin/python3.12", python_home)

        _ = shutil.copy2(runtime / "lib" / DYLIB_NAME, staged / DYLIB_NAME)
        _ = shutil.copytree(
            model_source,
            staged / "local-model",
            ignore=shutil.ignore_patterns(".DS_Store", "__pycache__", "*.pyc", "*.pyo"),
        )
        _ = (staged / "runtime-manifest.json").write_text(
            json.dumps(manifest, indent=2) + "\n",
            encoding="utf-8",
        )

        replacement = PACKAGING_DIR / "build.new"
        shutil.rmtree(replacement, ignore_errors=True)
        _ = shutil.copytree(staged, replacement)
        shutil.rmtree(BUILD_DIR, ignore_errors=True)
        _ = replacement.replace(BUILD_DIR)
        write_pyo3_config(BUILD_DIR, lock)

    print(f"Staged bundled Python runtime at {BUILD_DIR}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
