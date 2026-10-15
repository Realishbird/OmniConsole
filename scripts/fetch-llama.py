#!/usr/bin/env python3
"""Fetch llama.cpp's prebuilt server for one build target into src-tauri/binaries/llama.

Usage: fetch-llama.py <target name> <output dir> [release tag]

The target name is the CI matrix name (windows-x64, windows-arm64, macos-arm64,
macos-x64, linux-x64, linux-arm64). The newest llama.cpp release is used unless
a tag is given. GPU builds are preferred (Vulkan on Windows and Linux, Metal is
part of the macOS build); on Windows and Linux x64 the CPU build goes into the
sibling folder `llama-cpu/` as a fallback for machines without a working Vulkan driver.
When no asset matches a target the folder gets an UNAVAILABLE marker and the
app offers Ollama, LM Studio and cloud providers instead.
"""
import hashlib
import io
import json
import os
import re
import shutil
import sys
import tarfile
import urllib.request
import zipfile

REPO = "ggml-org/llama.cpp"

PREFERENCES = {
    "windows-x64": [r"bin-win-vulkan-x64\.(zip|tar\.gz)$", r"bin-win-cpu-x64\.(zip|tar\.gz)$", r"bin-win-avx2-x64\.(zip|tar\.gz)$", r"bin-win-x64\.(zip|tar\.gz)$"],
    "windows-arm64": [r"bin-win-vulkan-arm64\.(zip|tar\.gz)$", r"bin-win-cpu-arm64\.(zip|tar\.gz)$", r"bin-win-llvm-arm64\.(zip|tar\.gz)$", r"bin-win-arm64\.(zip|tar\.gz)$"],
    "macos-arm64": [r"bin-macos-arm64\.(zip|tar\.gz)$"],
    "macos-x64": [r"bin-macos-x64\.(zip|tar\.gz)$"],
    "linux-x64": [r"bin-ubuntu-vulkan-x64\.(zip|tar\.gz)$", r"bin-ubuntu-x64\.(zip|tar\.gz)$"],
    "linux-arm64": [r"bin-ubuntu-vulkan-arm64\.(zip|tar\.gz)$", r"bin-ubuntu-arm64\.(zip|tar\.gz)$"],
}
CPU_FALLBACK = {
    "windows-x64": [r"bin-win-cpu-x64\.(zip|tar\.gz)$", r"bin-win-avx2-x64\.(zip|tar\.gz)$"],
    "linux-x64": [r"bin-ubuntu-x64\.(zip|tar\.gz)$"],
    "linux-arm64": [r"bin-ubuntu-arm64\.(zip|tar\.gz)$"],
}
KEEP = re.compile(r"^(llama-server(\.exe)?|.*\.(dll|so|dylib|metal)|.*\.so\.\d+|LICENSE.*)$", re.IGNORECASE)


def api(url):
    req = urllib.request.Request(url, headers={"Accept": "application/vnd.github+json", "User-Agent": "OmniConsole-CI"})
    token = os.environ.get("GITHUB_TOKEN")
    if token:
        req.add_header("Authorization", f"Bearer {token}")
    with urllib.request.urlopen(req, timeout=60) as r:
        return json.load(r)


def download(url):
    req = urllib.request.Request(url, headers={"User-Agent": "OmniConsole-CI"})
    with urllib.request.urlopen(req, timeout=600) as r:
        return r.read()


def find_release(tag):
    """The tagged release, or the newest one that carries prebuilt binaries.

    llama.cpp's "latest" release can be a version tag with sources only; the
    binaries ride the b#### builds, so walk the list until one has them.
    """
    if tag:
        return api(f"https://api.github.com/repos/{REPO}/releases/tags/{tag}")
    for page in range(1, 6):
        for rel in api(f"https://api.github.com/repos/{REPO}/releases?per_page=30&page={page}"):
            if rel.get("draft"):
                continue
            if any(re.search(r"-bin-", a["name"]) for a in rel.get("assets", [])):
                return rel
    sys.exit("no llama.cpp release with prebuilt binaries found")


def pick(assets, patterns):
    for pat in patterns:
        for a in assets:
            if re.search(pat, a["name"]):
                return a
    return None


def extract(asset, dest):
    data = download(asset["browser_download_url"])
    digest = asset.get("digest") or ""
    if digest.startswith("sha256:"):
        actual = hashlib.sha256(data).hexdigest()
        if actual != digest.split(":", 1)[1]:
            sys.exit(f"checksum mismatch for {asset['name']}: expected {digest}, got sha256:{actual}")
        print(f"  verified sha256 of {asset['name']}")
    else:
        print(f"  no digest published for {asset['name']}; skipped verification")
    # (name, open-for-read) pairs for every regular file in the archive
    if asset["name"].endswith(".zip"):
        z = zipfile.ZipFile(io.BytesIO(data))
        entries = [(i.filename, lambda i=i: z.open(i)) for i in z.infolist() if not i.is_dir()]
    else:
        t = tarfile.open(fileobj=io.BytesIO(data), mode="r:gz")
        entries = [(m.name, lambda m=m: t.extractfile(m)) for m in t.getmembers() if m.isfile()]
    server = [n for n, _ in entries if re.search(r"(^|/)llama-server(\.exe)?$", n)]
    if not server:
        sys.exit(f"{asset['name']} has no llama-server")
    folder = server[0].rsplit("/", 1)[0] + "/" if "/" in server[0] else ""
    os.makedirs(dest, exist_ok=True)
    kept = 0
    for name, opener in entries:
        if not name.startswith(folder):
            continue
        rel = name[len(folder):]
        if "/" in rel or not KEEP.match(rel):
            continue
        with opener() as src, open(os.path.join(dest, rel), "wb") as out:
            shutil.copyfileobj(src, out)
        os.chmod(os.path.join(dest, rel), 0o755)
        kept += 1
    print(f"  {kept} files from {asset['name']} -> {dest}")


def main():
    if len(sys.argv) < 3:
        sys.exit(__doc__)
    target, out = sys.argv[1], sys.argv[2]
    tag = sys.argv[3] if len(sys.argv) > 3 else None
    os.makedirs(out, exist_ok=True)
    for n in os.listdir(out):
        if n != "README.txt":
            p = os.path.join(out, n)
            shutil.rmtree(p) if os.path.isdir(p) else os.remove(p)
    release = find_release(tag)
    assets = release.get("assets", [])
    print(f"llama.cpp {release['tag_name']}: {len(assets)} assets")
    main_asset = pick(assets, PREFERENCES.get(target, []))
    if not main_asset:
        with open(os.path.join(out, "UNAVAILABLE"), "w") as f:
            f.write(f"no llama.cpp {release['tag_name']} asset for {target}\n")
        print(f"no asset for {target}; the built-in runtime will be unavailable on this platform")
        print("assets in that release: " + ", ".join(a["name"] for a in assets[:60]))
        return
    print(f"runtime: {main_asset['name']}")
    extract(main_asset, out)
    # the CPU build goes in a sibling folder (binaries/llama-cpu): a subfolder would be
    # flattened into llama/ by the resource glob and make the MSI build fail on duplicates
    cpu_out = os.path.join(os.path.dirname(os.path.normpath(out)), "llama-cpu")
    os.makedirs(cpu_out, exist_ok=True)
    for n in os.listdir(cpu_out):
        if n != "README.txt":
            os.remove(os.path.join(cpu_out, n))
    fallback = pick(assets, CPU_FALLBACK.get(target, []))
    if fallback and fallback["name"] != main_asset["name"]:
        print(f"cpu fallback: {fallback['name']}")
        extract(fallback, cpu_out)
    with open(os.path.join(out, "VERSION"), "w") as f:
        f.write(release["tag_name"] + "\n")


if __name__ == "__main__":
    main()
