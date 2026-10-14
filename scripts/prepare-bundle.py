#!/usr/bin/env python3
import plistlib
import sys
import tomllib
from pathlib import Path
from release_config import ROOT, load_config

config = load_config()
info = plistlib.loads((ROOT / "assets/Info.plist").read_bytes())
info["CFBundleShortVersionString"] = tomllib.loads((ROOT / "Cargo.toml").read_text())["package"]["version"]
info["CFBundleVersion"] = str(config["build_number"])
if config["repository"]:
    info["SUFeedURL"] = config["repository"] + "/releases/latest/download/appcast.xml"
    info["SUPublicEDKey"] = config["public_key"]
else:
    print("Development build: updates remain inactive until the release repository is configured.")
Path(sys.argv[1]).write_bytes(plistlib.dumps(info))
