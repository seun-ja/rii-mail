#!/usr/bin/env python3
"""Apply an ad-hoc signature to all native code and the enclosing app bundle."""

from __future__ import annotations

import subprocess
import sys
from pathlib import Path


def is_macho(path: Path) -> bool:
    if not path.is_file() or path.is_symlink():
        return False
    result = subprocess.run(
        ["file", "-b", str(path)], check=True, text=True, capture_output=True
    )
    return "Mach-O" in result.stdout


def main() -> int:
    if len(sys.argv) != 2:
        print(f"usage: {Path(sys.argv[0]).name} /path/to/RiiMail.app", file=sys.stderr)
        return 2
    app = Path(sys.argv[1]).resolve()
    if not app.is_dir() or app.suffix != ".app":
        raise RuntimeError(f"not an app bundle: {app}")

    for metadata in app.rglob(".DS_Store"):
        metadata.unlink()

    executable = app / "Contents/MacOS/riimail"
    native_files = sorted(
        (path for path in app.rglob("*") if is_macho(path)),
        key=lambda path: (path == executable, -len(path.parts), str(path)),
    )
    for path in native_files:
        _ = subprocess.run(
            ["codesign", "--force", "--sign", "-", "--timestamp=none", str(path)],
            check=True,
            stdout=subprocess.DEVNULL,
        )

    _ = subprocess.run(
        [
            "codesign",
            "--force",
            "--deep",
            "--sign",
            "-",
            "--timestamp=none",
            str(app),
        ],
        check=True,
    )
    print(f"Ad-hoc signed {len(native_files)} Mach-O files and {app}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
