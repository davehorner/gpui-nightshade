"""Compare native backends using frozen assets and the full MCP model showcase."""
import argparse
import csv
import hashlib
import json
import math
from pathlib import Path
import random
import shutil
import statistics
import subprocess
import time

from cinematic import Mcp
import models


def phase(path, value):
    temporary = path.with_suffix(".tmp")
    temporary.write_text(value, encoding="utf-8")
    temporary.replace(path)


def gpu_environment():
    if not shutil.which("nvidia-smi"):
        return "nvidia-smi unavailable"
    return subprocess.check_output([
        "nvidia-smi", "--query-gpu=name,driver_version,pstate,utilization.gpu,memory.used,clocks.gr,clocks.mem,power.draw",
        "--format=csv"], text=True, timeout=10)


class BenchClient(Mcp):
    def __init__(self, url, control, views):
        super().__init__(url)
        self.control, self.views, self.role = control, views, "idle"
        self.playback = []

    def call(self, name, **arguments):
        if name == "screenshot":
            phase(self.control, "idle")
        return super().call(name, **arguments)

    def call_applied(self, name, **arguments):
        result = super().call_applied(name, **arguments)
        if name in ("set_animation", "play_clip"):
            self.playback.append({"role": self.role, "tool": name, "arguments": arguments})
        if name == "play_clip":
            # Give every non-cinematic view useful framing for each replacement model.
            for index in range(1, self.views):
                super().call_applied("focus_camera_view", index=index)
                super().call_applied("frame_model")
            super().call_applied("focus_camera_view", index=0)
            phase(self.control, self.role)
        return result


def summarize(path):
    groups = {}
    with path.open(newline="", encoding="utf-8") as stream:
        for row in csv.DictReader(stream):
            groups.setdefault(row["act"], []).append(row)
    result = {}
    for act, rows in groups.items():
        values = {key: sorted(float(r[key]) for r in rows) for key in
                  ("dispatch_ms", "render_complete_ms", "gpu_span_ms", "readback_ms", "total_ms")}
        result[act] = {"samples": len(rows), "distinct_sampled_pixel_hashes": len({r["pixel_hash"] for r in rows})}
        for key, numbers in values.items():
            result[act][key] = {"mean": statistics.mean(numbers), "median": statistics.median(numbers),
                                "p95": numbers[max(0, math.ceil(len(numbers)*0.95)-1)]}
        if len(rows) < 20 or result[act]["distinct_sampled_pixel_hashes"] < 2:
            raise RuntimeError(f"insufficient animated samples for {act}: {result[act]}")
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=Path("target/backend-benchmark"))
    parser.add_argument("--manifest", type=Path)
    parser.add_argument("--prepare-only", action="store_true")
    parser.add_argument("--seed", type=int, default=7)
    parser.add_argument("--duration", type=float, default=24)
    parser.add_argument("--fps", type=float, default=12)
    parser.add_argument("--width", type=int, default=1920)
    parser.add_argument("--height", type=int, default=1080)
    parser.add_argument("--views", type=int, choices=(1, 4), default=4)
    parser.add_argument("--order", default="dx12,vulkan,vulkan,dx12")
    parser.add_argument("--port", type=int, default=8850)
    args = parser.parse_args()
    args.output = args.output.resolve()
    args.output.mkdir(parents=True, exist_ok=True)
    if not math.isfinite(args.duration) or args.duration <= 2 or not math.isfinite(args.fps) or not 1 <= args.fps <= 60:
        parser.error("duration must exceed the two-second warmup; fps must be 1..60")
    if args.width <= 0 or args.height <= 0 or not 1 <= args.port <= 65535:
        parser.error("dimensions must be positive and port must be 1..65535")
    order = args.order.split(",")
    if any(backend not in ("dx12", "vulkan") for backend in order):
        parser.error("order must contain dx12 and/or vulkan")
    if args.manifest:
        manifest = json.loads(args.manifest.read_text(encoding="utf-8"))
    else:
        chosen, rng = models.select_models(args.seed, Path("target/mcp-cinematic/model-cache").resolve())
        manifest = {"seed": args.seed, "rng_state": rng.getstate(), "assets": [
            {"role": role, "name": asset["name"], "source": asset["source"],
             "path": str(asset["path"].resolve()), "sha256": hashlib.sha256(asset["path"].read_bytes()).hexdigest()}
            for role, asset in chosen]}
    (args.output/"manifest.json").write_text(json.dumps(manifest, indent=2), encoding="utf-8")
    print("Frozen assets:", ", ".join(a["name"] for a in manifest["assets"]), flush=True)
    if args.prepare_only:
        return
    configuration = {key: str(value) if isinstance(value, Path) else value for key, value in vars(args).items()}
    configuration["git_commit"] = subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip()
    configuration["rustc"] = subprocess.check_output(["rustc", "--version"], text=True).strip()
    (args.output/"configuration.json").write_text(json.dumps(configuration, indent=2), encoding="utf-8")
    executable = Path("target/release/examples/backend_benchmark.exe").resolve()
    results = []
    for number, backend in enumerate(order, 1):
        directory = args.output/f"run-{number:02}-{backend}"
        directory.mkdir(exist_ok=False)
        control = directory/"control.txt"
        phase(control, "idle")
        prefix = directory/"frames"
        (directory/"gpu-before.csv").write_text(gpu_environment(), encoding="utf-8")
        log = (directory/"host.log").open("w", encoding="utf-8")
        process = subprocess.Popen([str(executable), backend, str(args.port), str(args.width), str(args.height),
                                    str(args.views), str(control), str(prefix)], stdout=log, stderr=log)
        print(f"Run {number}/{len(order)}: {backend}", flush=True)
        try:
            client = BenchClient(f"http://127.0.0.1:{args.port}/", control, args.views)
            deadline = time.monotonic()+90
            while True:
                if process.poll() is not None:
                    raise RuntimeError(f"{backend} host exited: {directory/'host.log'}")
                try:
                    client.connect()
                    break
                except (OSError, RuntimeError):
                    if time.monotonic() >= deadline:
                        raise
                    time.sleep(.1)
            state = manifest["rng_state"]
            rng = random.Random()
            rng.setstate((state[0], tuple(state[1]), state[2]))
            for asset in manifest["assets"]:
                path = Path(asset["path"])
                if hashlib.sha256(path.read_bytes()).hexdigest() != asset["sha256"]:
                    raise RuntimeError(f"frozen asset changed: {path}")
                client.role = asset["role"]
                model_args = argparse.Namespace(scene=directory/"scene.gltf", duration=args.duration,
                                                fps=args.fps, screenshot=directory/"snapshot.png")
                models.show_model(client, {**asset, "path": path}, asset["role"], rng, model_args)
                phase(control, "idle")
            phase(control, "stop")
            if process.wait(timeout=30) != 0:
                raise RuntimeError(f"{backend} host failed: {directory/'host.log'}")
            summary = summarize(prefix.with_suffix(".csv"))
            if set(summary) != {a["role"] for a in manifest["assets"]}:
                raise RuntimeError(f"missing benchmark acts: {summary.keys()}")
            result = {"backend": backend, "run": number, "acts": summary, "playback": client.playback,
                      "metadata": json.loads(prefix.with_suffix(".json").read_text(encoding="utf-8"))}
            results.append(result)
            (directory/"gpu-after.csv").write_text(gpu_environment(), encoding="utf-8")
            if any(r["playback"] != results[0]["playback"] for r in results):
                raise RuntimeError("clip/speed choices differed between runs")
            (args.output/"results.json").write_text(json.dumps(results, indent=2), encoding="utf-8")
            print(f"Completed {backend}: {sum(a['samples'] for a in summary.values())} measured frames", flush=True)
        finally:
            if process.poll() is None:
                phase(control, "stop")
                try:
                    process.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    process.terminate()
                    process.wait(timeout=10)
            log.close()


if __name__ == "__main__":
    main()
