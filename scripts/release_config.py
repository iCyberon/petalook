import base64
import json
import re
from pathlib import Path
from urllib.parse import urlparse

ROOT = Path(__file__).resolve().parent.parent
CONFIG = ROOT / "release/updates.json"


def repository_url(value):
    parsed = urlparse(value)
    if (parsed.scheme != "https" or parsed.netloc != "github.com"
            or parsed.query or parsed.fragment
            or not re.fullmatch(r"/[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+/?", parsed.path)):
        raise ValueError("Use a GitHub repository URL: https://github.com/owner/repository")
    return value.rstrip("/").removesuffix(".git")


def public_key(value):
    if len(base64.b64decode(value, validate=True)) != 32:
        raise ValueError("The Sparkle public key must encode exactly 32 bytes")
    return value


def load_config(require_updates=False):
    config = json.loads(CONFIG.read_text())
    if type(config.get("build_number")) is not int or config["build_number"] < 2:
        raise ValueError("build_number must be an increasing integer, starting at 2")
    repository, key = config.get("repository"), config.get("public_key")
    if bool(repository) != bool(key):
        raise ValueError("Set both repository and public_key, or leave both unset")
    if repository:
        config["repository"] = repository_url(repository)
        config["public_key"] = public_key(key)
    elif require_updates:
        raise ValueError("Configure the GitHub repository and signing key before making an update release")
    return config
