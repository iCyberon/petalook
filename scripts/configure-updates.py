#!/usr/bin/env python3
import argparse
import json
import subprocess
from release_config import CONFIG, ROOT, load_config, public_key, repository_url

parser = argparse.ArgumentParser(description="Connect signed macOS updates to GitHub Releases")
parser.add_argument("repository", type=repository_url)
parser.add_argument("--public-key", type=public_key, help="Use an existing release signing public key")
args = parser.parse_args()
config = load_config()
if config.get("repository") and config["repository"] != args.repository:
    parser.error("The release channel is already configured; review release/updates.json before changing it")
subprocess.run(["bash", str(ROOT / "scripts/fetch-sparkle.sh")], check=True)
key = args.public_key
if key is None:
    generator = ROOT / "target/sparkle/2.10.0/bin/generate_keys"
    subprocess.run([str(generator), "--account", config["keychain_account"]], check=True)
    key = subprocess.check_output([str(generator), "--account", config["keychain_account"], "-p"], text=True).strip()
    public_key(key)
if config.get("public_key") and config["public_key"] != key:
    parser.error("Refusing to replace the existing update signing key")
config.update(repository=args.repository, public_key=key)
CONFIG.write_text(json.dumps(config, indent=2) + "\n")
print("Update feed:", args.repository + "/releases/latest/download/appcast.xml")
print("Only the public key is saved in the project; the private key stays in Keychain.")
