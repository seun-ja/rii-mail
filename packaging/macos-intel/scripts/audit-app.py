#!/usr/bin/env python3
"""Audit a bundled RiiMail.app for architecture and non-relocatable dependencies."""

from __future__ import annotations

import plistlib
import subprocess
import sys
from pathlib import Path
from typing import cast

ALLOWED_ABSOLUTE_PREFIXES = ("/System/Library/", "/usr/lib/")
FORBIDDEN_FRAGMENTS = ("/opt/homebrew", "/Users/", "/usr/local/")


def output(*command: str) -> str:
    return subprocess.run(command, check=True, text=True, capture_output=True).stdout


def macho_files(app: Path) -> list[Path]:
    result: list[Path] = []
    for path in app.rglob("*"):
        if not path.is_file() or path.is_symlink():
            continue
        description = output("file", "-b", str(path))
        if "Mach-O" in description:
            result.append(path)
    return result


def audit(app: Path) -> None:
    contents = app / "Contents"
    resources = contents / "Resources"
    executable = contents / "MacOS/riimail"
    dylib = contents / "Frameworks/libpython3.12.dylib"
    required = [
        executable,
        dylib,
        resources / "python/bin/python3.12",
        resources / "python/lib/python3.12/site-packages/torch/__init__.py",
        resources / "python/lib/python3.12/site-packages/transformers/__init__.py",
        resources / "local-model/config.json",
        resources / "llm/local_inference.py",
    ]
    missing = [str(path) for path in required if not path.exists()]
    if missing:
        raise RuntimeError("missing bundled files:\n" + "\n".join(missing))

    with (contents / "Info.plist").open("rb") as stream:
        raw_plist = cast(object, plistlib.load(stream))
    if not isinstance(raw_plist, dict):
        raise TypeError("Info.plist root must be a dictionary")
    plist = cast(dict[str, object], raw_plist)
    if plist.get("LSMinimumSystemVersion") != "14.0":
        raise RuntimeError("LSMinimumSystemVersion must be 14.0")

    native_files = macho_files(app)
    if not native_files:
        raise RuntimeError("no Mach-O files found")

    failures: list[str] = []
    for path in native_files:
        architectures = output("lipo", "-archs", str(path)).split()
        if architectures != ["x86_64"]:
            failures.append(f"{path}: expected x86_64 only, got {architectures}")

        dependencies = output("otool", "-L", str(path)).splitlines()[1:]
        for line in dependencies:
            dependency = line.strip().split(" (", 1)[0]
            if any(fragment in dependency for fragment in FORBIDDEN_FRAGMENTS):
                failures.append(f"{path}: forbidden dependency {dependency}")
            elif dependency.startswith("/") and not dependency.startswith(
                ALLOWED_ABSOLUTE_PREFIXES
            ):
                failures.append(f"{path}: non-system absolute dependency {dependency}")

    executable_dependencies = output("otool", "-L", str(executable))
    if "@rpath/libpython3.12.dylib" not in executable_dependencies:
        failures.append("main executable does not load @rpath/libpython3.12.dylib")
    executable_commands = output("otool", "-l", str(executable))
    if "@executable_path/../Frameworks" not in executable_commands:
        failures.append("main executable lacks the Frameworks LC_RPATH")

    dylib_id = output("otool", "-D", str(dylib))
    if "@rpath/libpython3.12.dylib" not in dylib_id:
        failures.append("bundled libpython has a non-relocatable install name")

    dot_store = list(app.rglob(".DS_Store"))
    if dot_store:
        failures.append(f"bundle contains .DS_Store files: {dot_store}")

    if failures:
        raise RuntimeError("bundle audit failed:\n" + "\n".join(failures))

    _ = subprocess.run(
        ["codesign", "--verify", "--deep", "--strict", "--verbose=2", str(app)],
        check=True,
    )
    print(f"Audited {len(native_files)} Mach-O files in {app}")


def main() -> int:
    if len(sys.argv) != 2:
        print(f"usage: {Path(sys.argv[0]).name} /path/to/RiiMail.app", file=sys.stderr)
        return 2
    app = Path(sys.argv[1]).resolve()
    if not app.is_dir() or app.suffix != ".app":
        raise RuntimeError(f"not an app bundle: {app}")
    audit(app)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
