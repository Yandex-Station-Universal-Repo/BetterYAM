#!/usr/bin/env python3
import argparse
import hashlib
import io
import json
import os
import posixpath
import shutil
import sys
import tarfile
import tempfile
import urllib.parse
import urllib.request
from pathlib import Path


def read_source(source: str) -> bytes:
    parsed = urllib.parse.urlparse(source)
    if parsed.scheme in ("http", "https"):
        with urllib.request.urlopen(source) as response:
            return response.read()
    return Path(source).read_bytes()


def resolve_url(index_source: str, base_url: str, asset: str) -> str:
    if urllib.parse.urlparse(asset).scheme:
        return asset
    if base_url:
        return urllib.parse.urljoin(base_url.rstrip("/") + "/", asset)
    parsed = urllib.parse.urlparse(index_source)
    if parsed.scheme in ("http", "https"):
        return urllib.parse.urljoin(index_source, asset)
    return str(Path(index_source).parent / asset)


def safe_member(name: str) -> Path:
    normalized = posixpath.normpath(name)
    if normalized.startswith("../") or normalized == ".." or normalized.startswith("/"):
        raise ValueError(f"unsafe archive member: {name}")
    return Path(normalized)


def extract_tar(content: bytes, destination: Path) -> None:
    with tarfile.open(fileobj=io.BytesIO(content), mode="r:*") as archive:
        links = []
        for member in archive.getmembers():
            target = destination / safe_member(member.name)
            if member.isdev():
                raise ValueError(f"unsupported archive member: {member.name}")
            if member.issym() or member.islnk():
                links.append(member)
                continue
            if member.isdir():
                target.mkdir(parents=True, exist_ok=True)
            elif member.isfile():
                target.parent.mkdir(parents=True, exist_ok=True)
                with archive.extractfile(member) as source, target.open("wb") as output:
                    if source is None:
                        raise ValueError(f"cannot read archive member: {member.name}")
                    shutil.copyfileobj(source, output)
                os.chmod(target, member.mode)
        root = destination.resolve()
        for member in links:
            target = destination / safe_member(member.name)
            if member.issym():
                link = target.parent / safe_member(member.linkname)
            else:
                link = destination / safe_member(member.linkname)
            resolved = link.resolve()
            try:
                resolved.relative_to(root)
            except ValueError as error:
                raise ValueError(f"unsafe link target: {member.name}") from error
            if not resolved.is_file():
                raise ValueError(f"unsupported link target: {member.name}")
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(resolved, target)
            os.chmod(target, member.mode or resolved.stat().st_mode)


def find_program(root: Path, package: dict) -> Path:
    check = package.get("check", "")
    destination = package.get("dest", "")
    if check and destination and check.startswith(destination.rstrip("/") + "/"):
        candidate = root / check[len(destination.rstrip("/")) + 1:]
        if candidate.is_file():
            return candidate
    binary = package.get("bin", "")
    matches = [path for path in root.rglob(binary) if path.is_file()] if binary else []
    if len(matches) == 1:
        return matches[0]
    if check:
        matches = [path for path in root.rglob(Path(check).name) if path.is_file()]
        if len(matches) == 1:
            return matches[0]
    raise ValueError("cannot determine executable; use --program")


def quote(value: str) -> str:
    return json.dumps(value, ensure_ascii=False)


def launch_details(root: Path, package: dict, program: Path) -> tuple[Path, str | None, list[str]]:
    """apkg packages may ship `<name>/<name>-run` and bundle libraries in `<prefix>/lib`."""
    libraries = [directory.as_posix() for directory in [program.parent.parent / "lib"] if len(program.parts) >= 2 and (root / directory).is_dir()]
    runner = Path(package["name"]) / f"{package['name']}-run"
    if (root / runner).is_file():
        return runner, "sh", libraries
    return program, None, libraries


def manifest(package: dict, program: Path, mode: str, interpreter: str | None, libraries: list[str]) -> str:
    description = package.get("description", "")
    launch_extra = ([f"interpreter = {quote(interpreter)}"] if interpreter else []) + ([f"library_paths = [{', '.join(quote(item) for item in libraries)}]"] if libraries else [])
    return "\n".join([
        f"id = {quote(package['name'])}",
        f"name = {quote(package['name'])}",
        f"version = {quote(str(package.get('version', '0.0.0')))}",
        'entry = "launch.lua"',
        f"description = {quote(description)}",
        'permissions = ["exec"]',
        "",
        "[launch]",
        'kind = "binary"',
        f'mode = "{mode}"',
        f"path = {quote(program.as_posix())}",
        *launch_extra,
        "",
        "[compatibility]",
        f"features = [{quote('arch.' + package.get('arch', 'aarch64'))}]",
        "",
    ])


def write_yap(source: Path, package: dict, program: Path, mode: str, output: Path) -> None:
    program, interpreter, libraries = launch_details(source, package, program)
    output.parent.mkdir(parents=True, exist_ok=True)
    with tarfile.open(output, "w:gz") as archive:
        def add_bytes(name: str, content: bytes, mode: int = 0o644) -> None:
            info = tarfile.TarInfo(name)
            info.size = len(content)
            info.mode = mode
            archive.addfile(info, io.BytesIO(content))

        add_bytes("plugin.toml", manifest(package, program, mode, interpreter, libraries).encode())
        add_bytes("launch.lua", b"")
        for path in sorted(source.rglob("*")):
            if not path.is_file():
                continue
            relative = path.relative_to(source).as_posix()
            info = archive.gettarinfo(str(path), arcname=relative)
            with path.open("rb") as file:
                archive.addfile(info, file)


def main() -> int:
    parser = argparse.ArgumentParser(description="Convert an apkg repository package to YAM .yap")
    parser.add_argument("--index", required=True, help="packages.json path or URL")
    parser.add_argument("--package", required=True, dest="name", help="package name from the index")
    parser.add_argument("--output", help="output .yap path")
    parser.add_argument("--asset-dir", help="directory containing downloaded .tar assets")
    parser.add_argument("--program", help="relative executable path inside the extracted package")
    parser.add_argument("--mode", choices=("oneshot", "service"), default="oneshot")
    parser.add_argument("--skip-checksum", action="store_true")
    args = parser.parse_args()
    index = json.loads(read_source(args.index))
    package = next((item for item in index.get("packages", []) if item.get("name") == args.name), None)
    if package is None:
        raise SystemExit(f"package not found: {args.name}")
    asset_source = str(Path(args.asset_dir) / package["asset"]) if args.asset_dir else package.get("url") or resolve_url(args.index, index.get("base_url", ""), package["asset"])
    content = read_source(asset_source)
    if package.get("md5") and not args.skip_checksum:
        actual = hashlib.md5(content).hexdigest()
        if actual != package["md5"]:
            raise SystemExit(f"checksum mismatch: expected {package['md5']}, got {actual}")
    with tempfile.TemporaryDirectory(prefix="yam-apkg-") as temporary:
        root = Path(temporary)
        extract_tar(content, root)
        program = Path(args.program) if args.program else find_program(root, package).relative_to(root)
        if not (root / program).is_file():
            raise SystemExit(f"program not found: {program}")
        output = Path(args.output) if args.output else Path(f"{package['name']}-{package.get('version', '0.0.0')}.yap")
        write_yap(root, package, program, args.mode, output)
        print(output)
    return 0


if __name__ == "__main__":
    sys.exit(main())
