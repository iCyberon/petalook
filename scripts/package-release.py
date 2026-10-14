#!/usr/bin/env python3
import plistlib
import os
import platform
import subprocess
import tempfile
from release_config import ROOT, load_config

config = load_config(require_updates=True)
subprocess.run(["bash", str(ROOT / "scripts/fetch-sparkle.sh")], cwd=ROOT, check=True)
signing_key = subprocess.check_output([
    str(ROOT / "target/sparkle/2.10.0/bin/generate_keys"),
    "--account", config["keychain_account"], "-p",
], text=True).strip()
if signing_key != config["public_key"]:
    raise ValueError("The Keychain signing key does not match the public key trusted by the app")
subprocess.run(["bash", str(ROOT / "scripts/bundle.sh")], cwd=ROOT, check=True)
app = ROOT / "dist/Petalook.app"
if profile := os.environ.get("PETALOOK_NOTARY_PROFILE"):
    if os.environ.get("PETALOOK_SIGNING_IDENTITY", "-") == "-":
        raise ValueError("Notarization requires a Developer ID signing identity")
    with tempfile.TemporaryDirectory(prefix="petalook-notary-") as temporary:
        submission = temporary + "/Petalook.zip"
        subprocess.run(["ditto", "-c", "-k", "--keepParent", str(app), submission], check=True)
        subprocess.run(["xcrun", "notarytool", "submit", submission, "--keychain-profile", profile, "--wait"], check=True)
        subprocess.run(["xcrun", "stapler", "staple", str(app)], check=True)
info = plistlib.loads((app / "Contents/Info.plist").read_bytes())
version = info["CFBundleShortVersionString"]
tag = "v" + version
destination = ROOT / "dist/releases" / tag
destination.mkdir(parents=True, exist_ok=False)
architecture = platform.machine()
archive = destination / ("Petalook-" + version + "-macos-" + architecture + ".zip")
subprocess.run(["ditto", "-c", "-k", "--sequesterRsrc", "--keepParent", str(app), str(archive)], check=True)
subprocess.run([
    str(ROOT / "target/sparkle/2.10.0/bin/generate_appcast"),
    "--account", config["keychain_account"],
    "--download-url-prefix", config["repository"] + "/releases/download/" + tag + "/",
    "--link", config["repository"], "--maximum-deltas", "0", str(destination),
], check=True)
print("Signed update artifacts:", destination)
print("Attach the ZIP and appcast.xml together to GitHub release", tag)
print("Nothing has been published.")
