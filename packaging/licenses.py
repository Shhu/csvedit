#!/usr/bin/env python3
"""Writes THIRD-PARTY-LICENSES.txt: the license of every crate built into
CSV Edit (on Windows and Linux), with its text, as their licenses ask for
any copy of the program. Crates whose license files read the same are
grouped. Run it again after any change to Cargo.lock.

Usage: python3 packaging/licenses.py   (from anywhere; needs cargo)
"""

import glob
import os
import re
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
TARGETS = ["x86_64-pc-windows-msvc", "x86_64-unknown-linux-gnu"]
OUT = os.path.join(ROOT, "THIRD-PARTY-LICENSES.txt")


def crates():
    """(name, version, license expression) of the normal dependencies."""
    found = {}
    for target in TARGETS:
        out = subprocess.run(
            ["cargo", "tree", "-e", "normal", "--prefix", "none", "--target", target,
             "--format", "{p}|{l}"],
            cwd=ROOT, check=True, capture_output=True, text=True,
        ).stdout
        for line in out.splitlines():
            package, _, license = line.partition("|")
            parts = package.split()
            if len(parts) < 2 or parts[0] == "csvedit":
                continue
            name, version = parts[0], parts[1].lstrip("v")
            found[(name, version)] = license.strip() or "unknown"
    return sorted((n, v, l) for (n, v), l in found.items())


def license_files(name, version):
    home = os.environ.get("CARGO_HOME", os.path.expanduser("~/.cargo"))
    dirs = glob.glob(os.path.join(home, "registry", "src", "*", f"{name}-{version}"))
    if not dirs:
        return []
    files = []
    for path in sorted(os.listdir(dirs[0])):
        if re.match(r"(?i)^(licen[cs]e|copying|notice|unlicense)", path):
            full = os.path.join(dirs[0], path)
            if os.path.isfile(full):
                files.append(full)
    return files


def main():
    groups = {}
    for name, version, expression in crates():
        texts = []
        for path in license_files(name, version):
            with open(path, encoding="utf-8", errors="replace") as f:
                texts.append(f"--- {os.path.basename(path)} ---\n{f.read().strip()}")
        text = "\n\n".join(texts) or f"(License: {expression}; no license file in the crate.)"
        # Same text, apart from spacing: one group.
        key = re.sub(r"\s+", " ", text)
        groups.setdefault(key, (text, []))[1].append(f"{name} {version} ({expression})")
    with open(OUT, "w", encoding="utf-8") as out:
        out.write(
            "CSV Edit is built with the Rust crates below. Their licenses ask that\n"
            "their notices go with every copy of the program: here they are, crates\n"
            "whose license files read the same grouped together. The fonts and icons\n"
            "are listed in THIRD-PARTY-NOTICES.md.\n"
        )
        for text, names in sorted(groups.values(), key=lambda g: g[1][0]):
            out.write("\n" + "=" * 76 + "\n")
            out.write("\n".join(names) + "\n\n")
            out.write(text + "\n")
    print(f"{OUT}: {sum(len(n) for _, n in groups.values())} crates, {len(groups)} texts")


if __name__ == "__main__":
    sys.exit(main())
