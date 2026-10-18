#!/usr/bin/env python3
"""Generate THIRD-PARTY-NOTICES.txt and a CycloneDX 1.5 SBOM (sbom.cdx.json) for the
TV build from `cargo metadata` plus the bundled/static C components.

Usage: gen-notices.py [--target armv7-unknown-linux-gnueabi] [--out-dir .]
"""
import argparse
import json
import os
import subprocess

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

C_COMPONENTS = [
    {"name": "SDL2 (webosbrew SDL-webOS backport)", "version": "2.30.12-webos.6", "license": "Zlib",
     "url": "https://github.com/webosbrew/SDL-webOS", "how": "shared library bundled in lib/"},
    {"name": "Inter typeface", "version": "4.1", "license": "OFL-1.1",
     "url": "https://rsms.me/inter", "how": "embedded in the executable"},
]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--target", default="armv7-unknown-linux-gnueabi")
    ap.add_argument("--out-dir", default=ROOT)
    ap.add_argument("--licenses-dir", default=None, help="copy every crate's LICENSE/COPYING/NOTICE files here")
    a = ap.parse_args()
    meta = json.loads(subprocess.check_output(
        ["cargo", "metadata", "--format-version", "1", "--locked", "--filter-platform", a.target],
        cwd=ROOT))
    # Only packages reachable from the app's resolve graph.
    nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}
    pkgs = {p["id"]: p for p in meta["packages"]}
    root = next(p["id"] for p in meta["packages"] if p["name"] == "luma-app")
    seen, stack = set(), [root]
    while stack:
        i = stack.pop()
        if i in seen:
            continue
        seen.add(i)
        for d in nodes[i]["deps"]:
            normal = any(k.get("kind") in (None, "normal") for k in d["dep_kinds"])
            # `*-src` build dependencies (e.g. openssl-src) compile C code into the binary.
            vendored = pkgs[d["pkg"]]["name"].endswith("-src")
            if normal or vendored:
                stack.append(d["pkg"])
    crates = sorted((pkgs[i] for i in seen), key=lambda p: (p["name"], p["version"]))

    lines = [
        "Luma Blocks - third-party notices",
        "=" * 34,
        "",
        "Luma Blocks is licensed under the MIT License (see LICENSE.txt).",
        "Source: https://github.com/danzig666/luma-blocks-webos",
        "",
        "Native components",
        "-----------------",
    ]
    for c in C_COMPONENTS:
        lines.append(f"* {c['name']} {c['version']} - {c['license']} - {c['url']} ({c['how']})")
    lines += ["", "Rust crates (name version - license - source)", "-" * 45]
    for p in crates:
        src = p.get("repository") or (p.get("source") or "workspace")
        lines.append(f"* {p['name']} {p['version']} - {p.get('license') or 'see source'} - {src}")
    lines += ["", "The full license texts of these components are available from their sources",
              "listed above; texts required to accompany binaries are included in licenses/."]
    with open(os.path.join(a.out_dir, "THIRD-PARTY-NOTICES.txt"), "w") as f:
        f.write("\n".join(lines) + "\n")

    if a.licenses_dir:
        import shutil
        n = 0
        for p in crates:
            d = os.path.dirname(p["manifest_path"])
            if p["name"] in ("luma-app", "luma-core", "luma-input"):
                continue
            dest = os.path.join(a.licenses_dir, "crates", f"{p['name']}-{p['version']}")
            for fn in sorted(os.listdir(d)):
                up = fn.upper()
                if up.startswith(("LICENSE", "LICENCE", "COPYING", "NOTICE", "COPYRIGHT")) and os.path.isfile(os.path.join(d, fn)):
                    os.makedirs(dest, exist_ok=True)
                    shutil.copyfile(os.path.join(d, fn), os.path.join(dest, fn))
                    n += 1
            # Bundled C sources inside -sys / -src crates.
            for sub in ("libsodium", "openssl", "src/libsodium"):
                sd = os.path.join(d, sub)
                if os.path.isdir(sd):
                    for fn in os.listdir(sd):
                        if fn.upper().startswith(("LICENSE", "COPYING")) and os.path.isfile(os.path.join(sd, fn)):
                            os.makedirs(dest, exist_ok=True)
                            shutil.copyfile(os.path.join(sd, fn), os.path.join(dest, sub.replace("/", "_") + "-" + fn))
                            n += 1
        print(f"copied {n} license files")
    comps = []
    for p in crates:
        c = {"type": "library", "name": p["name"], "version": p["version"],
             "purl": f"pkg:cargo/{p['name']}@{p['version']}"}
        if p.get("license"):
            c["licenses"] = [{"expression": p["license"]}]
        comps.append(c)
    for c in C_COMPONENTS:
        comps.append({"type": "library", "name": c["name"], "version": c["version"],
                      "licenses": [{"expression": c["license"]}],
                      "externalReferences": [{"type": "website", "url": c["url"]}]})
    sbom = {"bomFormat": "CycloneDX", "specVersion": "1.5", "version": 1,
            "metadata": {"component": {"type": "application", "name": "luma-blocks",
                                       "version": next(p["version"] for p in meta["packages"] if p["name"] == "luma-app")}},
            "components": comps}
    with open(os.path.join(a.out_dir, "sbom.cdx.json"), "w") as f:
        json.dump(sbom, f, indent=1)
    print(f"{len(crates)} crates, {len(C_COMPONENTS)} native components")


if __name__ == "__main__":
    main()
