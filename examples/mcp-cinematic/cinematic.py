#!/usr/bin/env python3
"""A dependency-free, real-time MCP choreography client. See README.md."""

import argparse
import base64
import colorsys
import json
import math
from pathlib import Path
import random
import struct
import time
import urllib.error
import urllib.request


def mix(a, b, t):
    return [x + (y - x) * t for x, y in zip(a, b)]


def ease(t):
    t = max(0.0, min(1.0, t))
    return t * t * t * (t * (t * 6 - 15) + 10)


def spline(points, t):
    """Clamped Catmull-Rom path, evaluated on [0, 1]."""
    at = max(0.0, min(1.0, t)) * (len(points) - 1)
    i = min(int(at), len(points) - 2)
    u = at - i
    a, b, c, d = [points[max(0, min(len(points) - 1, j))]
                  for j in (i - 1, i, i + 1, i + 2)]
    return [0.5 * (2 * y + (-x + z) * u
                   + (2 * x - 5 * y + 4 * z - w) * u * u
                   + (-x + 3 * y - 3 * z + w) * u * u * u)
            for x, y, z, w in zip(a, b, c, d)]


def look_at(eye, target):
    """Viewer XYZ Euler degrees; glTF cameras look along local -Z."""
    x, y, z = [b - a for a, b in zip(eye, target)]
    return [math.degrees(math.atan2(y, math.hypot(x, z))),
            math.degrees(math.atan2(-x, -z)), 0.0]


def choreography(seed, count):
    rng = random.Random(seed)
    objects = []
    for i in range(count):
        angle = math.tau * i / count + rng.uniform(-0.15, 0.15)
        radius = rng.uniform(2.0, 3.8)
        home = [radius * math.cos(angle), rng.uniform(0.9, 1.8),
                radius * math.sin(angle)]
        objects.append({
            "name": f"Cinematic object {i + 1:02d}",
            "home": home,
            "lift": [home[0] * 1.25, rng.uniform(2.3, 4.8), home[2] * 1.25],
            "size": rng.uniform(0.45, 0.85),
            "spin": [rng.uniform(40, 140), rng.uniform(100, 260), rng.uniform(-70, 70)],
            "phase": rng.uniform(0, math.tau),
            "proportions": [rng.uniform(0.7, 1.2), rng.uniform(0.8, 1.6), rng.uniform(0.7, 1.2)],
        })
    return objects


def object_pose(obj, index, count, t):
    # Staggered rise, floating turn, then a soft landing. No frame accumulation.
    delay = 0.12 * index / max(1, count - 1)
    rise = ease((t - 0.06 - delay) / 0.24)
    settle = ease((t - 0.73 - delay * 0.4) / 0.22)
    lift = rise * (1 - settle)
    position = mix(obj["home"], obj["lift"], lift)
    position[1] += math.sin(t * math.tau * 2 + obj["phase"]) * 0.18 * lift
    twist = ease((t - 0.12) / 0.76)
    rotation = [v * twist for v in obj["spin"]]
    scale = obj["size"] * (1 + 0.12 * math.sin(math.pi * lift))
    return {"translation": position, "rotation": rotation, "scale": [scale * v for v in obj["proportions"]]}


def camera_pose(t):
    # Establishing view -> low lateral dolly -> elevated reveal -> close landing.
    position = spline([[11, 7, 15], [9, 3.6, 9], [1, 5.8, 11],
                       [-9, 7.5, 7], [-7, 3.8, 8]], ease(t))
    target = [0, 1.4 + 0.8 * math.sin(math.pi * t), 0]
    return {"translation": position, "rotation": look_at(position, target)}


def geometry(shape):
    if shape == "box":
        vertices = [[x, y, z] for x in (-1, 1) for y in (-1, 1) for z in (-1, 1)]
        faces = [(0, 1, 3), (0, 3, 2), (4, 6, 7), (4, 7, 5),
                 (0, 4, 5), (0, 5, 1), (2, 3, 7), (2, 7, 6),
                 (0, 2, 6), (0, 6, 4), (1, 5, 7), (1, 7, 3)]
    elif shape == "diamond":
        vertices = [[0, 1.4, 0], [0, -1.4, 0], [1, 0, 0], [0, 0, 1],
                    [-1, 0, 0], [0, 0, -1]]
        faces = [(0, 3, 2), (0, 4, 3), (0, 5, 4), (0, 2, 5),
                 (1, 2, 3), (1, 3, 4), (1, 4, 5), (1, 5, 2)]
    else:
        vertices = [[-1, -0.7, -1], [1, -0.7, -1], [1, -0.7, 1],
                    [-1, -0.7, 1], [0, 1.4, 0]]
        faces = [(0, 4, 1), (1, 4, 2), (2, 4, 3), (3, 4, 0), (0, 2, 3), (0, 1, 2)]
    positions, normals = [], []
    for face in faces:
        a, b, c = [vertices[i] for i in face]
        u, v = [[q - p for p, q in zip(a, point)] for point in (b, c)]
        normal = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2],
                  u[0] * v[1] - u[1] * v[0]]
        length = math.sqrt(sum(x * x for x in normal))
        for point in (a, b, c):
            positions.extend(point)
            normals.extend(x / length for x in normal)
    return positions, normals


def write_scene(path, objects, shape="diamond"):
    """Self-contained glTF: shared faceted geometry and a free camera."""
    blob = bytearray()
    doc = {"asset": {"version": "2.0", "generator": "gpui-nightshade MCP cinematic"},
           "scene": 0, "scenes": [{"name": f"Cinematic scene {time.time_ns()}", "nodes": list(range(len(objects) + 1))}],
           "nodes": [], "meshes": [], "materials": [], "accessors": [], "bufferViews": [],
           "cameras": [{"name": "Cinematic camera", "type": "perspective",
                        "perspective": {"yfov": math.radians(44), "znear": 0.05, "zfar": 200}}]}

    def attribute(values):
        view = len(doc["bufferViews"])
        doc["bufferViews"].append({"buffer": 0, "byteOffset": len(blob),
                                   "byteLength": len(values) * 4, "target": 34962})
        blob.extend(struct.pack("<" + "f" * len(values), *values))
        index = len(doc["accessors"])
        doc["accessors"].append({"bufferView": view, "componentType": 5126,
                                 "count": len(values) // 3, "type": "VEC3",
                                 "min": [min(values[i::3]) for i in range(3)],
                                 "max": [max(values[i::3]) for i in range(3)]})
        return index

    # A shared mesh avoids the pinned renderer's distinct-mesh draw issue.
    # Flat vertex colors make each face readable without external lighting assets.
    positions, normals = geometry(shape)
    colors = []
    for face in range(len(positions) // 9):
        color = colorsys.hsv_to_rgb((0.5 + face * 0.075) % 1, 0.55, 0.72)
        colors.extend(list(color) * 3)
    doc["extensionsUsed"] = ["KHR_materials_unlit"]
    doc["meshes"].append({"name": "Cinematic shared crystal", "primitives": [{
        "attributes": {"POSITION": attribute(positions), "NORMAL": attribute(normals),
                       "COLOR_0": attribute(colors)}, "material": 0}]})
    doc["materials"].append({"name": "Cinematic facets", "extensions": {"KHR_materials_unlit": {}},
        "pbrMetallicRoughness": {"baseColorFactor": [1, 1, 1, 1],
                                 "metallicFactor": 0, "roughnessFactor": 0.5}})
    for obj in objects:
        doc["nodes"].append({"name": obj["name"], "mesh": 0, "translation": obj["home"],
                             "scale": [obj["size"] * v for v in obj["proportions"]]})
    doc["nodes"].append({"name": "Cinematic camera", "camera": 0, "translation": [11, 7, 15]})
    doc["buffers"] = [{"byteLength": len(blob), "uri": "data:application/octet-stream;base64,"
                       + base64.b64encode(blob).decode("ascii")}]
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(doc), encoding="utf-8")
    return doc["scenes"][0]["name"]


class Mcp:
    def __init__(self, url):
        self.url, self.sequence, self.calls = url, 0, 0

    def rpc(self, method, params):
        self.sequence += 1
        payload = {"jsonrpc": "2.0", "id": self.sequence, "method": method, "params": params}
        request = urllib.request.Request(self.url, json.dumps(payload, allow_nan=False).encode(),
                                         {"Content-Type": "application/json"})
        with urllib.request.urlopen(request, timeout=25) as response:
            answer = json.load(response)
        if answer.get("id") != self.sequence or "error" in answer:
            raise RuntimeError(f"{method}: {answer}")
        return answer["result"]

    def connect(self):
        self.rpc("initialize", {"protocolVersion": "2024-11-05", "capabilities": {},
                                "clientInfo": {"name": "mcp-cinematic", "version": "1"}})
        notification = {"jsonrpc": "2.0", "method": "notifications/initialized"}
        request = urllib.request.Request(self.url, json.dumps(notification).encode(),
                                         {"Content-Type": "application/json"})
        with urllib.request.urlopen(request, timeout=25) as response:
            response.read()
        tools = self.rpc("tools/list", {})["tools"]
        required = {"load_file", "get_scene", "get_viewer_state", "set_transform",
                    "set_active_camera", "set_transforms", "set_render", "set_handle_mode", "select_entity"}
        missing = required - {tool["name"] for tool in tools}
        if missing:
            raise RuntimeError(f"Viewer is missing tools: {sorted(missing)}")

    def call(self, name, **arguments):
        result = self.rpc("tools/call", {"name": name, "arguments": arguments})
        self.calls += 1
        if result.get("isError"):
            raise RuntimeError(f"{name}: {result.get('structuredContent', result)}")
        if "structuredContent" in result:
            return result["structuredContent"]
        return json.loads(result["content"][0]["text"])


def wait_for_scene(client, objects, scene_name, timeout=30):
    deadline = time.monotonic() + timeout
    expected = {obj["name"] for obj in objects} | {"Cinematic camera", scene_name}
    while time.monotonic() < deadline:
        nodes = client.call("get_scene")["nodes"]
        by_name = {node["name"]: node["entity"] for node in nodes}
        if expected <= by_name.keys():
            return by_name
        time.sleep(0.1)
    raise RuntimeError("Scene did not load within 30 seconds; check the viewer status.")


def run(args):
    objects = choreography(args.seed, args.objects)
    scene = args.scene.resolve()
    scene_name = write_scene(scene, objects, args.shape)
    print(f"Scene: {scene}\nSeed: {args.seed}; objects: {args.objects}", flush=True)
    if args.dry_run:
        print(json.dumps({"camera_midpoint": camera_pose(0.5),
                          "object_midpoint": object_pose(objects[0], 0, len(objects), 0.5)}, indent=2))
        return
    client = Mcp(args.url)
    client.connect()
    client.call("load_file", path=str(scene))
    entities = wait_for_scene(client, objects, scene_name)
    camera = entities["Cinematic camera"]
    client.call("set_render", grid=False, turntable=False, bloom=True, exposure=0.7,
                atmosphere=3, sky=True, shading=0, pbr_debug=0, fog=False,
                depth_of_field=False, normals=False, bounds=False, skeleton=False)
    client.call("set_handle_mode", mode="none")
    client.call("select_entity")
    client.call("set_transform", entity=camera, **camera_pose(0))
    client.call("set_active_camera", entity=camera)
    deadline = time.monotonic() + 5
    while client.call("get_viewer_state")["active_camera"] != camera:
        if time.monotonic() > deadline:
            raise RuntimeError("The cinematic camera was not activated.")
        time.sleep(0.05)
    print("Playing: rise / floating turn / dolly reveal / landing. Ctrl+C freezes the scene.", flush=True)
    started, frames = time.monotonic(), 0
    while True:
        tick = time.monotonic()
        t = min(1.0, (tick - started) / args.duration)
        transforms = [{"entity": camera, **camera_pose(t)}]
        transforms.extend({"entity": entities[obj["name"]],
                           **object_pose(obj, i, len(objects), t)}
                          for i, obj in enumerate(objects))
        client.call("set_transforms", transforms=transforms)
        frames += 1
        if t >= 1:
            break
        time.sleep(max(0, 1 / args.fps - (time.monotonic() - tick)))
    elapsed = time.monotonic() - started
    print(f"Finished {frames} updates in {elapsed:.2f}s ({frames / elapsed:.1f} Hz); "
          f"{client.calls} MCP tool calls. Final scene remains open.")
    if args.screenshot:
        output = args.screenshot.resolve()
        output.parent.mkdir(parents=True, exist_ok=True)
        client.call("screenshot", path=str(output))
        deadline = time.monotonic() + 10
        while not output.is_file():
            if time.monotonic() > deadline:
                raise RuntimeError(f"Screenshot was queued but did not appear: {output}")
            time.sleep(0.1)
        print(f"Screenshot: {output}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--url", default="http://127.0.0.1:8791/")
    parser.add_argument("--seed", type=int, default=7)
    parser.add_argument("--shape", choices=("diamond", "box", "pyramid"), default="diamond")
    parser.add_argument("--objects", type=int, default=6)
    parser.add_argument("--duration", type=float, default=24)
    parser.add_argument("--fps", type=float, default=12)
    parser.add_argument("--scene", type=Path, default=Path("target/mcp-cinematic/scene.gltf"))
    parser.add_argument("--screenshot", type=Path)
    parser.add_argument("--dry-run", action="store_true", help="Generate the scene without contacting MCP")
    args = parser.parse_args()
    if not 1 <= args.objects <= 32:
        parser.error("--objects must be between 1 and 32")
    if not math.isfinite(args.duration) or args.duration <= 0:
        parser.error("--duration must be finite and positive")
    if not math.isfinite(args.fps) or not 1 <= args.fps <= 60:
        parser.error("--fps must be between 1 and 60")
    if args.screenshot and args.screenshot.exists():
        parser.error("--screenshot must name a new file")
    try:
        run(args)
    except KeyboardInterrupt:
        print("\nStopped; the viewer retains the last pose.")
    except (RuntimeError, OSError, urllib.error.URLError) as error:
        parser.exit(1, f"Cinematic example failed: {error}\n")


if __name__ == "__main__":
    main()
