import base64
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("release_config", Path(__file__).resolve().parents[1] / "scripts/release_config.py")
config = importlib.util.module_from_spec(spec)
spec.loader.exec_module(config)


class ReleaseConfigTests(unittest.TestCase):
    def test_repository_requires_https_github_without_credentials(self):
        for url in ("http://github.com/team/app", "https://github.com.evil.test/team/app",
                    "https://user:secret@github.com/team/app", "https://github.com/team/app?token=secret",
                    "https://github.com/team/app/releases"):
            with self.subTest(url=url), self.assertRaises(ValueError):
                config.repository_url(url)
        self.assertEqual(config.repository_url("https://github.com/team/app.git"), "https://github.com/team/app")

    def test_public_key_requires_exact_ed25519_length(self):
        for key in ("not base64", base64.b64encode(bytes(31)).decode(), base64.b64encode(bytes(64)).decode()):
            with self.subTest(key=key), self.assertRaises(ValueError):
                config.public_key(key)

    def test_incomplete_configuration_cannot_ship_as_an_update(self):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / "updates.json"
            with patch.object(config, "CONFIG", path):
                path.write_text(json.dumps(dict(build_number=2, repository=None, public_key=None)))
                self.assertIsNone(config.load_config()["repository"])
                with self.assertRaises(ValueError):
                    config.load_config(require_updates=True)
                path.write_text(json.dumps(dict(build_number=2, repository="https://github.com/team/app", public_key=None)))
                with self.assertRaises(ValueError):
                    config.load_config()


if __name__ == "__main__":
    unittest.main()
