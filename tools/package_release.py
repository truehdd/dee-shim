#!/usr/bin/env python3
"""Package a native release build and its CLI help. Requires Python 3.11+."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import struct
import subprocess
import tarfile
import tomllib
import zipfile


# DEE 6.7 for Linux needs glibc 2.34; dee_shim must not need more. The linker takes each symbol's
# newest version from the build host, so a newer build host or dependency can raise the floor
# silently.
GLIBC_MAX = (2, 34)


def glibc_versions(path):
    """GLIBC_x.y versions an x86_64 ELF requires, read from its .gnu.version_r section."""
    data = Path(path).read_bytes()
    if data[:4] != b"\x7fELF" or data[4] != 2 or data[5] != 1:
        raise SystemExit(f"{path}: not a little-endian ELF64 file")
    shoff, = struct.unpack_from("<Q", data, 0x28)
    shentsize, shnum = struct.unpack_from("<HH", data, 0x3A)
    sections = [struct.unpack_from("<IIQQQQIIQQ", data, shoff + i * shentsize) for i in range(shnum)]
    found = set()
    for _, sh_type, _, _, offset, _, link, count, _, _ in sections:
        if sh_type != 0x6FFFFFFE:  # SHT_GNU_verneed
            continue
        strtab = sections[link][4]
        need = offset
        for _ in range(count):
            _, aux_count, _, aux, next_need = struct.unpack_from("<HHIII", data, need)
            entry = need + aux
            for _ in range(aux_count):
                _, _, _, name, next_aux = struct.unpack_from("<IHHII", data, entry)
                end = data.index(b"\0", strtab + name)
                text = data[strtab + name:end].decode()
                if text.startswith("GLIBC_2."):
                    found.add(tuple(int(part) for part in text[6:].split(".")))
                entry += next_aux
            need += next_need
    return found


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--build-dir", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--commit", required=True)
    parser.add_argument("--platform", choices=["linux", "macos", "windows"], required=True)
    args = parser.parse_args()

    manifest = Path(__file__).resolve().parents[1] / "Cargo.toml"
    version = tomllib.loads(manifest.read_text())["workspace"]["package"]["version"]
    if len(args.commit) != 40 or any(c not in "0123456789abcdef" for c in args.commit):
        parser.error("--commit must be the full lowercase Git commit hash")

    if args.platform == "linux":
        versions = glibc_versions(args.build_dir / "dee_shim")
        newest = max(versions, default=(0,))
        if newest > GLIBC_MAX:
            parser.error("dee_shim needs glibc " + ".".join(map(str, newest)) + ", above the "
                         + ".".join(map(str, GLIBC_MAX)) + " that DEE 6.7 requires")

    # Panic locations carry source paths; a release build remaps the build host's home and checkout
    # (as .github/workflows/release.yml does) so neither reaches the binary.
    binary = (args.build_dir / ("dee_shim.exe" if args.platform == "windows" else "dee_shim")).read_bytes()
    for leak in {str(Path.home()), str(Path(__file__).resolve().parents[1])}:
        if leak.encode() in binary:
            parser.error(f"dee_shim contains the build path {leak}; build with --remap-path-prefix")

    name = f"dee-shim-{version}-{args.commit[:7]}-{args.platform}-x86_64"
    output = args.output.resolve()
    package = output / name
    package.mkdir(parents=True, exist_ok=False)
    windows = args.platform == "windows"
    command = "dee_shim.exe" if windows else "dee_shim"
    shutil.copy2(args.build_dir / command, package / command)
    for notice in ["LICENSE", "NOTICE", "THIRD_PARTY_LICENSES.txt", "CHANGELOG.md"]:
        shutil.copy2(manifest.parent / notice, package / notice)

    cli = package / "cli"
    cli.mkdir()
    env = dict(os.environ, NO_COLOR="1", COLUMNS="100")
    commands = [([], "help")]
    tools = ["ddp_encoder", "ddpjoc_encoder", "dthd_encoder"]
    commands += [([tool], tool) for tool in tools]
    for subcommand, filename in commands:
        result = subprocess.run(
            [str(package / command), *subcommand, "--help"],
            check=True, capture_output=True, text=True, env=env,
        )
        (cli / f"{filename}.txt").write_text(result.stdout, encoding="utf-8")
    for tool in tools:
        result = subprocess.run(
            [str(package / command), tool, "--list"],
            check=True, capture_output=True, text=True, env=env,
        )
        (cli / f"{tool}-layouts.txt").write_text(result.stdout, encoding="utf-8")

    (package / "README.txt").write_text(
        f"dee-shim {version} development snapshot\nCommit: {args.commit}\n\n"
        f"Run {command}. It runs the DEE tool inside its own process; place it\n"
        "beside the DEE tools or pass --binary. The cli/ directory contains\n"
        "help and layout lists captured from this build. DEE executables and\n"
        "licences are not included. All binaries are x86_64; Apple Silicon\n"
        "needs Rosetta 2. Options depend on the selected DEE profile.\n",
        encoding="utf-8",
    )
    (package / "BUILD.json").write_text(json.dumps({
        "version": version, "commit": args.commit,
        "platform": args.platform, "architecture": "x86_64",
    }, indent=2) + "\n", encoding="utf-8")
    files = sorted(p for p in package.rglob("*") if p.is_file())
    (package / "SHA256SUMS").write_text("".join(
        hashlib.sha256(p.read_bytes()).hexdigest() + "  "
        + p.relative_to(package).as_posix() + "\n" for p in files
    ), encoding="utf-8")

    archive = output / (name + (".zip" if windows else ".tar.gz"))
    if windows:
        with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED) as zipped:
            for path in sorted(package.rglob("*")):
                if path.is_file():
                    zipped.write(path, path.relative_to(output).as_posix())
    else:
        with tarfile.open(archive, "w:gz") as tar:
            tar.add(package, arcname=name)
    print(archive)


if __name__ == "__main__":
    main()
