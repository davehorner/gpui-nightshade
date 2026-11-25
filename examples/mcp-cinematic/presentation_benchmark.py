"""Benchmark visible Vulkan presentation with the frozen four-act MCP showcase."""
import argparse
import bisect
import hashlib
import json
import math
import os
from pathlib import Path
import random
import re
import socket
import statistics
import subprocess
import time

from benchmark import BenchClient, gpu_environment, phase
import models


class PresentationClient(BenchClient):
    def __init__(self, url, control, views):
        super().__init__(url, control, views)
        self.intervals = []

    def call_applied(self, name, **arguments):
        result = super().call_applied(name, **arguments)
        if name == "play_clip":
            self.intervals.append({"role": self.role, "start_ns": time.perf_counter_ns()+2_000_000_000})
        return result

    def call(self, name, **arguments):
        if name == "screenshot":
            self.intervals[-1]["end_ns"] = time.perf_counter_ns()
        return super().call(name, **arguments)


def distribution(values):
    values = sorted(values)
    if not values:
        raise RuntimeError("no timing samples")
    return {"mean": statistics.mean(values), "median": statistics.median(values),
            "p95": values[math.ceil(len(values)*.95)-1]}


def summarize(log, intervals, mode):
    events = []
    contents = log.read_text(encoding="utf-8")
    if re.search(r"panicked at|validation error|device lost", contents, re.IGNORECASE):
        raise RuntimeError(f"GPU/runtime failure in {log}")
    for line in contents.splitlines():
        if not line.startswith("NSBENCH "):
            continue
        row = dict(field.split("=", 1) for field in line.split()[1:] if "=" in field)
        if row["event"] == "paint_error":
            raise RuntimeError(line)
        events.append(row)
    metadata = [e for e in events if e["event"] == "meta"]
    expected = "VulkanShared" if mode == "vulkan-shared" else "Readback"
    if len(metadata) != 1 or metadata[0].get("graphics") != "Vulkan" or metadata[0].get("transport") != expected:
        raise RuntimeError(f"unexpected backend: {metadata}")
    timed = sorted((e for e in events if "ns" in e), key=lambda e: int(e["ns"]))
    adopts = [e for e in timed if e["event"] == "adopt"]
    adopt_times = [int(e["ns"]) for e in adopts]
    first_presents = []
    seen = set()
    for present in (e for e in timed if e["event"] == "present"):
        index = bisect.bisect_right(adopt_times, int(present["ns"]))-1
        if index < 0:
            continue
        adopted = adopts[index]
        key = (adopted["generation"], adopted["sequence"])
        if key not in seen:
            seen.add(key)
            first_presents.append({**adopted, "ns": present["ns"], "age_ms":
                float(adopted["age_ms"])+(int(present["ns"])-int(adopted["ns"]))/1e6})
    results = {}
    for interval in intervals:
        start, end = interval["start_ns"], interval["end_ns"]
        seconds = (end-start)/1e9
        def select(source, event=None):
            return [e for e in source if (event is None or e["event"] == event)
                    and start <= int(e["ns"]) < end
                    and int(e["ns"])-float(e.get("age_ms", 0))*1e6 >= start]
        published = select(timed, "publish")
        adopted = select(adopts)
        presented = select(first_presents)
        presents = select(timed, "present")
        dimensions = sorted({(int(e["width"]), int(e["height"])) for e in published})
        if len(dimensions) != 1 or min(len(published), len(adopted), len(presented)) < 20:
            raise RuntimeError(f"insufficient/stale/resized samples: {interval}, {dimensions}")
        results[interval["role"]] = {
            "seconds": seconds, "dimensions": dimensions[0],
            "published_frames": len(published), "adopted_frames": len(adopted),
            "unique_presented_frames": len(presented), "present_submissions": len(presents),
            "published_fps": len(published)/seconds, "adopted_fps": len(adopted)/seconds,
            "unique_presented_fps": len(presented)/seconds, "present_submission_fps": len(presents)/seconds,
            "render_cpu_ms": distribution([float(e["render_cpu_ms"]) for e in published]),
            "publication_age_ms": distribution([float(e["age_ms"]) for e in published]),
            "adoption_age_ms": distribution([float(e["age_ms"]) for e in adopted]),
            "first_present_age_ms": distribution([float(e["age_ms"]) for e in presented])}
    return results


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--duration", type=float, default=24)
    parser.add_argument("--order", default="vulkan-shared,readback,readback,vulkan-shared")
    parser.add_argument("--port", type=int, default=8851)
    args = parser.parse_args()
    order = args.order.split(",")
    if not 1 <= args.port <= 65535:
        parser.error("port must be 1..65535")
    if args.duration <= 2 or not math.isfinite(args.duration) or any(m not in ("vulkan-shared", "readback") for m in order):
        parser.error("duration must exceed two seconds; order accepts vulkan-shared/readback")
    args.output = args.output.resolve()
    args.output.mkdir(parents=True, exist_ok=False)
    manifest = json.loads(args.manifest.read_text(encoding="utf-8"))
    for asset in manifest["assets"]:
        if hashlib.sha256(Path(asset["path"]).read_bytes()).hexdigest() != asset["sha256"]:
            raise RuntimeError(f"asset changed: {asset['path']}")
    (args.output/"manifest.json").write_text(json.dumps(manifest, indent=2), encoding="utf-8")
    executable = Path("target/release/gpui-nightshade-viewer.exe").resolve()
    configuration = {"duration": args.duration, "warmup_seconds": 2, "order": order, "views": 4,
                     "mcp_fps": 12, "clock": "Windows QPC nanoseconds / Python perf_counter_ns",
                     "executable_sha256": hashlib.sha256(executable.read_bytes()).hexdigest(),
                     "git_commit": subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip()}
    (args.output/"configuration.json").write_text(json.dumps(configuration, indent=2), encoding="utf-8")
    results = []
    for number, mode in enumerate(order, 1):
        with socket.socket() as probe:
            probe.bind(("127.0.0.1", args.port))
        directory = args.output/f"run-{number:02}-{mode}"
        directory.mkdir()
        control = directory/"control.txt"
        phase(control, "idle")
        (directory/"gpu-before.csv").write_text(gpu_environment(), encoding="utf-8")
        env = {**os.environ, "WGPU_BACKEND": "vulkan", "GPUI_NIGHTSHADE_BENCHMARK": "1",
               "GPUI_NIGHTSHADE_LAYOUT": str(directory/"layout.json")}
        print(f"Run {number}/{len(order)}: {mode}", flush=True)
        with (directory/"viewer.log").open("w", encoding="utf-8") as log:
            process = subprocess.Popen([str(executable),
                f"--presentation={mode}", "--camera-views=4", f"--agent-port={args.port}"],
                stdout=log, stderr=log, env=env, creationflags=subprocess.CREATE_NO_WINDOW)
            try:
                client = PresentationClient(f"http://127.0.0.1:{args.port}/", control, 4)
                deadline = time.monotonic()+90
                while True:
                    if process.poll() is not None:
                        raise RuntimeError(f"viewer exited: {directory}")
                    try:
                        client.connect()
                        break
                    except (OSError, RuntimeError):
                        if time.monotonic() > deadline:
                            raise
                        time.sleep(.1)
                state = manifest["rng_state"]
                rng = random.Random()
                rng.setstate((state[0], tuple(state[1]), state[2]))
                for asset in manifest["assets"]:
                    client.role = asset["role"]
                    models.show_model(client, {**asset, "path": Path(asset["path"])}, asset["role"], rng,
                        argparse.Namespace(scene=directory/"scene.gltf", duration=args.duration, fps=12,
                                           screenshot=directory/"snapshot.png"))
                (directory/"intervals.json").write_text(json.dumps(client.intervals, indent=2), encoding="utf-8")
            finally:
                if process.poll() is None:
                    process.terminate()
                process.wait(timeout=10)
        acts = summarize(directory/"viewer.log", client.intervals, mode)
        result = {"run": number, "mode": mode, "acts": acts, "playback": client.playback}
        results.append(result)
        if any(r["playback"] != results[0]["playback"] for r in results):
            raise RuntimeError("playback choices differ")
        if len({tuple(a["dimensions"]) for r in results for a in r["acts"].values()}) != 1:
            raise RuntimeError("viewport dimensions differ")
        (directory/"gpu-after.csv").write_text(gpu_environment(), encoding="utf-8")
        (args.output/"results.json").write_text(json.dumps(results, indent=2), encoding="utf-8")
        print(f"Completed {mode}: unique presented FPS " +
              ", ".join(f"{role}={a['unique_presented_fps']:.1f}" for role, a in acts.items()), flush=True)


if __name__ == "__main__":
    main()
