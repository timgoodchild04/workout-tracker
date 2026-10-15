"""Finish a release built by the parallel jobs: write latest.json (what the in-app
updater reads) from the uploaded .sig files, then publish the draft.

Refuses to publish unless macOS, Linux (AppImage + .deb) and Windows are all there,
so the updater never sees a release missing a platform.

  GH_TOKEN=... python3 publish_release.py OWNER/REPO RELEASE_ID [--dry-run]
"""
import json, os, sys, urllib.request
from datetime import datetime, timezone

API = "https://api.github.com"

def call(url, method="GET", data=None, accept="application/vnd.github+json", ctype=None):
    req = urllib.request.Request(url, data=data, method=method)
    req.add_header("Authorization", f"Bearer {os.environ['GH_TOKEN']}")
    req.add_header("Accept", accept)
    req.add_header("X-GitHub-Api-Version", "2022-11-28")
    if ctype:
        req.add_header("Content-Type", ctype)
    with urllib.request.urlopen(req) as r:
        body = r.read()
    return body

# Which updater platform keys each built file serves (the same keys tauri-action writes).
PLATFORMS = [
    (".app.tar.gz", ["darwin-aarch64", "darwin-x86_64", "darwin-aarch64-app", "darwin-x86_64-app"]),
    (".AppImage", ["linux-x86_64", "linux-x86_64-appimage"]),
    (".deb", ["linux-x86_64-deb"]),
    ("-setup.exe", ["windows-x86_64", "windows-x86_64-nsis"]),
]

def updater_json(repo, release):
    assets = {a["name"]: a for a in release["assets"]}
    tag = release["tag_name"]
    out = {"version": tag.lstrip("v"), "notes": release.get("body") or "", "pub_date": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%S.%f")[:-3] + "Z", "platforms": {}}
    for suffix, keys in PLATFORMS:
        files = [n for n in assets if n.endswith(suffix)]
        if len(files) != 1 or files[0] + ".sig" not in assets:
            sys.exit(f"missing or ambiguous {suffix} (+ .sig) in {tag}: {sorted(assets)}")
        sig = call(assets[files[0] + ".sig"]["url"], accept="application/octet-stream").decode().strip()
        for k in keys:
            out["platforms"][k] = {"signature": sig, "url": f"https://github.com/{repo}/releases/download/{tag}/{files[0]}"}
    return out

def main():
    repo, release_id = sys.argv[1], sys.argv[2]
    dry = "--dry-run" in sys.argv
    release = json.loads(call(f"{API}/repos/{repo}/releases/{release_id}"))
    doc = updater_json(repo, release)
    print(json.dumps(doc, indent=2))
    if dry:
        return
    for a in release["assets"]:
        if a["name"] == "latest.json":
            call(f"{API}/repos/{repo}/releases/assets/{a['id']}", method="DELETE")
    call(f"https://uploads.github.com/repos/{repo}/releases/{release_id}/assets?name=latest.json",
         method="POST", data=json.dumps(doc, indent=2).encode(), ctype="application/json")
    call(f"{API}/repos/{repo}/releases/{release_id}", method="PATCH",
         data=json.dumps({"draft": False, "make_latest": "true"}).encode(), ctype="application/json")
    print(f"published {release['tag_name']}", file=sys.stderr)

if __name__ == "__main__":
    main()
