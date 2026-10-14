#!/usr/bin/env python3
"""Repeated process-launch to first image-frame measurements (warm OS caches)."""
import argparse
import json
import os
from pathlib import Path
import re
import selectors
import signal
import statistics
import subprocess
import tempfile
import time

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("executable", type=Path)
parser.add_argument("images", type=Path, nargs="+")
parser.add_argument("--runs", type=int, default=5)
parser.add_argument("--output", type=Path, required=True)
parser.add_argument("--launch-services", action="store_true",
                    help="Pass an .app bundle; open files via macOS Open With delivery")
args = parser.parse_args()
measurements = []
for image in args.images:
    samples = []
    for run in range(args.runs + 1):
        stages = {}
        logs = []
        app_pid = None
        temporary = tempfile.TemporaryDirectory(prefix="petalook-startup-")
        trace_path = Path(temporary.name) / "trace.log"
        trace_path.touch()
        started = time.perf_counter()
        if args.launch_services:
            process = subprocess.Popen(
                ["open", "-n", "-W", "-a", str(args.executable.resolve()),
                 "--stderr", str(trace_path), "--env", "PETALOOK_STARTUP_TRACE=1",
                 str(image.resolve())],
                stdout=subprocess.DEVNULL, stderr=subprocess.PIPE,
            )
            stream = trace_path.open("rb")
        else:
            process = subprocess.Popen(
                [str(args.executable.resolve()), str(image.resolve())],
                env={**os.environ, "PETALOOK_STARTUP_TRACE": "1"},
                stdout=subprocess.DEVNULL, stderr=subprocess.PIPE,
            )
            stream = process.stderr
        selector = selectors.DefaultSelector()
        if not args.launch_services:
            selector.register(stream, selectors.EVENT_READ)
        buffer = b""
        try:
            while time.perf_counter() - started < 15:
                if not args.launch_services and not selector.select(timeout=0.25):
                    if process.poll() is not None:
                        break
                    continue
                chunk = os.read(stream.fileno(), 65536)
                if not chunk:
                    if args.launch_services and process.poll() is None:
                        time.sleep(0.001)
                        continue
                    break
                buffer += chunk
                while b"\n" in buffer:
                    line, buffer = buffer.split(b"\n", 1)
                    text = line.decode(errors="replace")
                    logs.append(text)
                    if text.startswith("startup_pid: "):
                        app_pid = int(text.split(": ")[1])
                    match = re.match(r"startup (\w+): ([\d.]+) ms", text)
                    if match:
                        stages.setdefault(match[1], float(match[2]))
                if "first_media_frame" in stages:
                    stages["process_to_frame"] = (time.perf_counter() - started) * 1000
                    # Allow the measured frame to reach the compositor before teardown.
                    time.sleep(0.08)
                    break
        finally:
            selector.close()
            if args.launch_services and app_pid is not None:
                try:
                    os.kill(app_pid, signal.SIGTERM)
                except ProcessLookupError:
                    pass
                try:
                    process.wait(timeout=3)
                except subprocess.TimeoutExpired:
                    pass
            if process.poll() is None:
                process.terminate()
            try:
                process.wait(timeout=3)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()
            stream.close()
            if args.launch_services:
                process.stderr.close()
            temporary.cleanup()
        if "process_to_frame" not in stages:
            raise RuntimeError(f"No media frame for {image}: {logs}")
        if run:
            samples.append(stages)
        # LaunchServices also tears down Dock/application state after process exit.
        time.sleep(2 if args.launch_services else 0.15)
    medians = {key: round(statistics.median(s[key] for s in samples if key in s), 2)
               for key in samples[0]}
    result = {"image": str(image), "runs": samples, "median_ms": medians}
    measurements.append(result)
    print(json.dumps({"image": image.name, "median_ms": medians}), flush=True)
args.output.parent.mkdir(parents=True, exist_ok=True)
args.output.write_text(json.dumps(measurements, indent=2) + "\n")
