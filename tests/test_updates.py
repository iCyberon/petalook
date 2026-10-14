#!/usr/bin/env python3
"""Exercise the real Sparkle download/verify/install cycle on disposable app bundles."""
import base64
import functools
import http.server
import os
from pathlib import Path
import plistlib
import shutil
import subprocess
import tempfile
import threading
import time
import uuid
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parent.parent
SPARKLE = ROOT / "target/sparkle/2.10.0"


def run(*args, **kwargs):
    return subprocess.run([str(x) for x in args], check=True, **kwargs)


def wait_for(predicate, message, timeout=45):
    end = time.monotonic() + timeout
    while time.monotonic() < end:
        if predicate():
            return
        time.sleep(0.1)
    raise AssertionError(message)


def bundle_version(app):
    return plistlib.loads((app / "Contents/Info.plist").read_bytes())["CFBundleVersion"]


with tempfile.TemporaryDirectory(prefix="petalook-updater-") as temporary:
    work = Path(temporary)
    keyfile = work / "test-key"
    keyfile.touch(mode=0o600)
    swift = work / "key.swift"
    swift.write_text('''import CryptoKit
import Foundation
let key = Curve25519.Signing.PrivateKey()
try key.rawRepresentation.base64EncodedString().write(toFile: CommandLine.arguments[1], atomically: false, encoding: .utf8)
print(key.publicKey.rawRepresentation.base64EncodedString())
''')
    public = subprocess.check_output(["swift", str(swift), str(keyfile)], text=True).strip()
    binary = work / "UpdaterProbe"
    run("xcrun", "clang", "-fobjc-arc", "-Wno-incompatible-pointer-types", "-F" + str(SPARKLE),
        ROOT / "tests/updater_probe.m", "-framework", "AppKit", "-o", binary)
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), functools.partial(
        http.server.SimpleHTTPRequestHandler, directory=str(work)))
    threading.Thread(target=server.serve_forever, daemon=True).start()
    try:
        for corrupt in (False, True):
            case = work / ("invalid-signature" if corrupt else "valid-update")
            case.mkdir()
            identifier = "am.mkrtchyan.petalook.updatertest." + uuid.uuid4().hex
            app = case / "installed/UpdaterProbe.app"
            resources = app / "Contents"
            (resources / "MacOS").mkdir(parents=True)
            (resources / "Frameworks").mkdir()
            shutil.copy2(binary, resources / "MacOS/UpdaterProbe")
            run("ditto", SPARKLE / "Sparkle.framework", resources / "Frameworks/Sparkle.framework")
            info = {
                "CFBundleIdentifier": identifier, "CFBundleName": "UpdaterProbe",
                "CFBundleExecutable": "UpdaterProbe", "CFBundlePackageType": "APPL",
                "CFBundleVersion": "1", "CFBundleShortVersionString": "1.0",
                "NSPrincipalClass": "NSApplication", "LSMinimumSystemVersion": "12.0",
                "SUFeedURL": f"http://127.0.0.1:{server.server_port}/{case.name}/appcast.xml",
                "SUPublicEDKey": public, "SUEnableAutomaticChecks": True,
                "SUAutomaticallyUpdate": True, "SUVerifyUpdateBeforeExtraction": True,
                "NSAppTransportSecurity": {"NSAllowsLocalNetworking": True},
            }
            plist = resources / "Info.plist"
            plist.write_bytes(plistlib.dumps(info))
            run("codesign", "--force", "--deep", "--sign", "-", app, stderr=subprocess.DEVNULL)
            updated = case / "release/UpdaterProbe.app"
            run("ditto", app, updated)
            info.update(CFBundleVersion="2", CFBundleShortVersionString="2.0")
            (updated / "Contents/Info.plist").write_bytes(plistlib.dumps(info))
            run("codesign", "--force", "--deep", "--sign", "-", updated, stderr=subprocess.DEVNULL)
            archive = case / "update.zip"
            run("ditto", "-c", "-k", "--keepParent", updated, archive)
            signature = subprocess.check_output([str(SPARKLE / "bin/sign_update"), "--ed-key-file",
                str(keyfile), "-p", str(archive)], text=True).strip()
            if corrupt:
                signature = base64.b64encode(bytes(64)).decode()
            namespace = "http://www.andymatuschak.org/xml-namespaces/sparkle"
            ET.register_namespace("sparkle", namespace)
            rss = ET.Element("rss", version="2.0")
            channel = ET.SubElement(rss, "channel")
            ET.SubElement(channel, "title").text = "Updater test"
            item = ET.SubElement(channel, "item")
            ET.SubElement(item, "title").text = "Version 2"
            ET.SubElement(item, "{" + namespace + "}version").text = "2"
            ET.SubElement(item, "{" + namespace + "}shortVersionString").text = "2.0"
            ET.SubElement(item, "enclosure", {
                "url": f"http://127.0.0.1:{server.server_port}/{case.name}/update.zip",
                "length": str(archive.stat().st_size), "type": "application/octet-stream",
                "{" + namespace + "}edSignature": signature,
            })
            ET.ElementTree(rss).write(case / "appcast.xml", encoding="utf-8", xml_declaration=True)
            with (case / "client.log").open("w") as log:
                process = subprocess.Popen([str(resources / "MacOS/UpdaterProbe"), str(case)], stdout=log, stderr=log)
                try:
                    marker = case / ("error" if corrupt else "ready")
                    wait_for(marker.exists, "Updater did not finish: " + (case / "client.log").read_text())
                    assert bundle_version(app) == "1", "Installed an update while the app was still running"
                    if corrupt:
                        assert not (case / "ready").exists(), "Accepted an invalid signature"
                    else:
                        assert (case / "ready").read_text() == "2.0"
                    (case / "quit").touch()
                    process.wait(timeout=10)
                    if not corrupt:
                        wait_for(lambda: app.exists() and bundle_version(app) == "2", "Update was not installed on quit")
                        print("PASS: automatic download, staged while running, installed on quit", flush=True)
                    else:
                        assert bundle_version(app) == "1"
                        print("PASS: invalid signature rejected; original app preserved", flush=True)
                except Exception:
                    print((case / "client.log").read_text())
                    raise
                finally:
                    if process.poll() is None:
                        (case / "quit").touch()
                        try:
                            process.wait(timeout=5)
                        except subprocess.TimeoutExpired:
                            process.terminate()
                            process.wait(timeout=5)
                    run("defaults", "delete", identifier, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    finally:
        server.shutdown()
