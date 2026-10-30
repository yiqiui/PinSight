#!/usr/bin/env python3
"""Re-stage and zip the portable build using the already-built release exe.

Produces two packages:
  - PinSight-0.1.0-portable.zip       (plain exe)
  - PinSight-0.1.0-portable-upx.zip   (UPX-compressed exe, ~3x smaller; some
                                       antivirus engines flag UPX-packed binaries,
                                       which is why the plain one stays the default)

Mirrors scripts/package.ps1 staging (minus the cargo build):
  - pinsight.exe (from target/release)
  - README.md, LICENSE, config.example.json
  - assets/ (recursive)
  - pinsight.portable marker
  - captures/ dir
The runtime `data/` folder is excluded (it is created on first launch).
"""
import os
import shutil
import subprocess
import zipfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DIST = os.path.join(ROOT, "dist")
RELEASE_EXE = os.path.join(ROOT, "target", "release", "pinsight.exe")

UPX_CANDIDATES = [
    os.path.join(ROOT, "..", "..", "..", "kaifa", "upx-5.1.0-win64", "upx.exe"),
    r"D:\workspace\kaifa\upx-5.1.0-win64\upx.exe",
    "upx",
]


def find_upx():
    for cand in UPX_CANDIDATES:
        if os.path.isfile(cand):
            return cand
        if cand == "upx":
            found = shutil.which("upx")
            if found:
                return found
    return None


def stage_and_zip(exe_path, zip_name, stage_subdir):
    stage = os.path.join(DIST, stage_subdir, "PinSight")
    if os.path.isdir(stage):
        shutil.rmtree(stage)
    os.makedirs(stage, exist_ok=True)

    shutil.copy2(exe_path, os.path.join(stage, "pinsight.exe"))
    for f in ("README.md", "LICENSE", "NOTICE", "config.example.json"):
        src = os.path.join(ROOT, f)
        if os.path.isfile(src):
            shutil.copy2(src, stage)
    assets_src = os.path.join(ROOT, "assets")
    if os.path.isdir(assets_src):
        shutil.copytree(assets_src, os.path.join(stage, "assets"))

    with open(os.path.join(stage, "pinsight.portable"), "w", encoding="utf-8") as fh:
        fh.write("delete this file to install")
    os.makedirs(os.path.join(stage, "captures"), exist_ok=True)

    zip_path = os.path.join(DIST, zip_name)
    if os.path.isfile(zip_path):
        os.remove(zip_path)
    with zipfile.ZipFile(zip_path, "w", zipfile.ZIP_DEFLATED) as zf:
        for base, _dirs, files in os.walk(stage):
            for name in sorted(files):
                full = os.path.join(base, name)
                arc = os.path.relpath(full, os.path.join(stage, ".."))
                zf.write(full, arc)

    exe_size = os.path.getsize(os.path.join(stage, "pinsight.exe"))
    zip_size = os.path.getsize(zip_path)
    print(f"{zip_name}")
    print(f"  exe : {exe_size:,} bytes ({exe_size/1e6:.1f} MB)")
    print(f"  zip : {zip_size:,} bytes ({zip_size/1e6:.1f} MB)")
    return zip_path


# 1. Plain package (skipped when PINSIGHT_UPX_ONLY=1).
upx_only = os.environ.get("PINSIGHT_UPX_ONLY", "").strip().lower() in ("1", "true", "yes")
if not upx_only:
    stage_and_zip(RELEASE_EXE, "PinSight-0.1.0-portable.zip", "portable")

# 2. UPX package (optional).
upx = find_upx()
if upx:
    tmp = os.path.join(DIST, "_pinsight_upx.exe")
    shutil.copy2(RELEASE_EXE, tmp)
    try:
        subprocess.run(
            [upx, "--best", "--lzma", "-q", tmp],
            check=True,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        stage_and_zip(tmp, "PinSight-0.1.0-portable-upx.zip", "portable-upx")
        print("note: UPX-packed exe may trip some antivirus heuristics.")
    finally:
        if os.path.isfile(tmp):
            os.remove(tmp)
else:
    print("upx not found — skipped the -upx package")
    if upx_only:
        print("PINSIGHT_UPX_ONLY was set but upx is missing; building the plain package instead.")
        stage_and_zip(RELEASE_EXE, "PinSight-0.1.0-portable.zip", "portable")
