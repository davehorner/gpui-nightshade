"""Remote-first animated model showcase. Uses only Python's standard library."""

import base64
import json
import math
from pathlib import Path
import random
import struct
import time
import urllib.request

from cinematic import Mcp, camera_pose, wait_for_animation


ROOT = Path(__file__).resolve().parents[2]
ASSETS = Path(__file__).resolve().parent / "assets"
BASE = "https://raw.githubusercontent.com/KhronosGroup/"
CATALOG = {
    "cube": [("AnimatedMorphCube", BASE + "glTF-Sample-Assets/main/Models/AnimatedMorphCube/glTF-Binary/AnimatedMorphCube.glb")],
    "spring": [],
    "morph": [
        ("AnimatedMorphCube", BASE + "glTF-Sample-Assets/main/Models/AnimatedMorphCube/glTF-Binary/AnimatedMorphCube.glb"),
        ("AnimatedMorphSphere", BASE + "glTF-Sample-Models/master/2.0/AnimatedMorphSphere/glTF-Binary/AnimatedMorphSphere.glb"),
    ],
    "skeleton": [
        ("Fox", BASE + "glTF-Sample-Assets/main/Models/Fox/glTF-Binary/Fox.glb"),
        ("CesiumMan", BASE + "glTF-Sample-Assets/main/Models/CesiumMan/glTF-Binary/CesiumMan.glb"),
    ],
}
LOCAL = {
    "cube": [ASSETS / "AnimatedMorphCube.glb"],
    "spring": [ASSETS / "CoiledSpring.glb"],
    "morph": [ASSETS / "AnimatedMorphCube.glb"],
    "skeleton": [ROOT / "upstream/nightshade/assets/gltf" / name
                 for name in ("fox.glb", "x_bot.glb", "y_bot_locomotion.glb")],
}
MAX_BYTES = 32 * 1024 * 1024


def capability(role):
    return {"cube": "morph", "spring": "skeleton"}.get(role, role)


def parse_glb(data, role):
    role = capability(role)
    if len(data) < 20 or struct.unpack_from("<III", data) != (0x46546C67, 2, len(data)):
        raise ValueError("not a complete glTF 2 GLB")
    chunks, offset = {}, 12
    while offset < len(data):
        if offset + 8 > len(data):
            raise ValueError("truncated GLB chunk")
        length, kind = struct.unpack_from("<II", data, offset)
        offset += 8
        if offset + length > len(data):
            raise ValueError("truncated GLB data")
        chunks[kind] = data[offset:offset + length]
        offset += length
    doc = json.loads(chunks[0x4E4F534A])
    if not doc.get("animations"):
        raise ValueError("model has no animation clips")
    if any(b.get("uri") for b in doc.get("buffers", [])) or any(i.get("uri") for i in doc.get("images", [])):
        raise ValueError("example requires a self-contained GLB")
    if role == "skeleton":
        skinned = [n for n in doc.get("nodes", []) if "skin" in n and "mesh" in n]
        if not any(s.get("joints") for s in doc.get("skins", [])) or not any(
                {"JOINTS_0", "WEIGHTS_0"} <= p["attributes"].keys()
                for n in skinned for p in doc["meshes"][n["mesh"]]["primitives"]):
            raise ValueError("model has no bound skeleton with vertex joint weights")
    if role == "morph" and not any(c["target"]["path"] == "weights"
                                  for a in doc["animations"] for c in a["channels"]):
        raise ValueError("model has no animated morph-weight tracks")
    return doc, chunks.get(0x004E4942, b"")


def download(url, timeout):
    with urllib.request.urlopen(url, timeout=timeout) as response:
        data = response.read(MAX_BYTES + 1)
    if len(data) > MAX_BYTES:
        raise ValueError("model exceeds 32 MiB download limit")
    return data


def resolve_pool(role, cache, timeout=8, fetch=download):
    """Resolve availability first. RNG selection happens only after this returns."""
    cache.mkdir(parents=True, exist_ok=True)
    remote = []
    for name, url in CATALOG[role]:
        try:
            data = fetch(url, timeout)
            parse_glb(data, role)
            path = cache / (name + ".glb")
            temporary = path.with_suffix(".download")
            temporary.write_bytes(data)
            temporary.replace(path)
            remote.append({"name": name, "path": path, "source": "remote", "url": url})
        except (OSError, ValueError, KeyError) as error:
            print(f"Remote unavailable: {name}: {error}", flush=True)
    if remote:
        return remote
    local = []
    paths = [cache / (name + ".glb") for name, _ in CATALOG[role]] + LOCAL[role]
    seen = set()
    for path in paths:
        if not path.is_file() or path.resolve() in seen:
            continue
        try:
            parse_glb(path.read_bytes(), role)
        except (OSError, ValueError, KeyError) as error:
            print(f"Local rejected: {path}: {error}", flush=True)
            continue
        seen.add(path.resolve())
        local.append({"name": path.stem, "path": path, "source": "local fallback"})
    if not local:
        raise RuntimeError(f"No valid remote or local {role} model is available")
    return local


def select_models(seed, cache, timeout=8, fetch=download):
    # Resolve both pools before consuming randomness. A seed reproduces choices
    # for the same available pools; network availability can change those pools.
    pools = {role: resolve_pool(role, cache, timeout, fetch) for role in CATALOG}
    rng = random.Random(seed)
    chosen, names = [], set()
    for role, pool in pools.items():
        asset = pool[0] if role in ("cube", "spring") else rng.choice(pool)
        key = asset["name"].casefold()
        if key not in names:
            chosen.append((role, asset))
            names.add(key)
    return chosen, rng


def model_bounds(doc):
    """Scene-space bind-pose bounds, including node TRS/matrices (not camera nodes)."""
    identity = [float(i % 5 == 0) for i in range(16)]
    def multiply(a, b):
        return [sum(a[k * 4 + row] * b[col * 4 + k] for k in range(4))
                for col in range(4) for row in range(4)]
    points = []
    def visit(index, parent):
        node = doc["nodes"][index]
        if "matrix" in node:
            local = node["matrix"]
        else:
            x, y, z, w = node.get("rotation", [0, 0, 0, 1])
            sx, sy, sz = node.get("scale", [1, 1, 1])
            tx, ty, tz = node.get("translation", [0, 0, 0])
            local = [(1-2*y*y-2*z*z)*sx, (2*x*y+2*z*w)*sx, (2*x*z-2*y*w)*sx, 0,
                     (2*x*y-2*z*w)*sy, (1-2*x*x-2*z*z)*sy, (2*y*z+2*x*w)*sy, 0,
                     (2*x*z+2*y*w)*sz, (2*y*z-2*x*w)*sz, (1-2*x*x-2*y*y)*sz, 0,
                     tx, ty, tz, 1]
        world = multiply(parent, local)
        if "mesh" in node:
            for primitive in doc["meshes"][node["mesh"]]["primitives"]:
                accessor = doc["accessors"][primitive["attributes"]["POSITION"]]
                lo, hi = accessor["min"], accessor["max"]
                for corner in range(8):
                    p = [hi[i] if corner & (1 << i) else lo[i] for i in range(3)] + [1]
                    points.append([sum(world[k*4+r]*p[k] for k in range(4)) for r in range(3)])
        for child in node.get("children", []):
            visit(child, world)
    for node in doc["scenes"][doc.get("scene", 0)]["nodes"]:
        visit(node, identity)
    lo = [min(p[i] for p in points) for i in range(3)]
    hi = [max(p[i] for p in points) for i in range(3)]
    return [(a+b)/2 for a, b in zip(lo, hi)], max(max(b-a for a, b in zip(lo, hi)), 0.01)


def prepare_scene(asset, role, output):
    doc, binary = parse_glb(asset["path"].read_bytes(), role)
    center, size = model_bounds(doc)
    if binary:
        doc["buffers"][0]["uri"] = "data:application/octet-stream;base64," + base64.b64encode(binary).decode()
    marker = f"Model showcase {role} {time.time_ns()}"
    scene = doc["scenes"][doc.get("scene", 0)]
    scene["name"] = marker
    index = len(doc["nodes"])
    camera_index = len(doc.setdefault("cameras", []))
    doc["cameras"].append({"type": "perspective", "perspective": {
        "yfov": math.radians(44), "znear": size * 0.001}})
    doc["nodes"].append({"name": marker + " camera", "camera": camera_index})
    scene["nodes"].append(index)
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(doc), encoding="utf-8")
    return marker, center, size


def orbit_pose(t, center, size):
    pose = camera_pose(t)
    # Original camera path surrounds a roughly eight-unit-wide cast.
    pose["translation"] = [c + p * size / 6 for c, p in zip(center, pose["translation"])]
    # Recompute aim for the selected asset's actual center.
    from cinematic import look_at
    pose["rotation"] = look_at(pose["translation"], center)
    return pose


def show_model(client, asset, role, rng, args):
    path = args.scene.resolve().with_name(f"showcase-{role}.gltf")
    marker, center, size = prepare_scene(asset, role, path)
    choice = "Included required" if role in ("cube", "spring") else "RNG chose"
    print(f"{role}: {choice} {asset['name']} ({asset['source']}); {asset['path']}", flush=True)
    client.call("load_file", path=str(path))
    deadline = time.monotonic() + 30
    while True:
        nodes = client.call("get_scene")["nodes"]
        camera = next((n["entity"] for n in nodes if n["name"] == marker + " camera"), None)
        if camera is not None and any(n["name"] == marker for n in nodes):
            state = client.call("get_animation")
            if state["clips"]:
                break
        if time.monotonic() > deadline:
            raise RuntimeError(f"{asset['name']} did not load with native animation clips")
        time.sleep(0.1)
    clip = rng.choice(state["clips"])
    print(f"  RNG clip: {clip['name']} (index {clip['index']}); "
          f"skeleton joints: {sum(len(s['joints']) for s in parse_glb(asset['path'].read_bytes(), role)[0].get('skins', []))}", flush=True)
    client.call("select_entity")
    client.call("set_render", skeleton=capability(role) == "skeleton", turntable=False, grid=False,
                atmosphere=1, sky=True, sun_intensity=4, exposure=0.9, shading=0, pbr_debug=0)
    client.call("set_transform", entity=camera, **orbit_pose(0, center, size))
    client.call("set_active_camera", entity=camera)
    client.call_applied("set_animation", speed=rng.uniform(0.75, 1.25), looping=True, blend_time=0)
    client.call_applied("play_clip", index=clip["index"], blend_time=0)
    wait_for_animation(client, lambda a: a["playing"] and a["time"] > 0)
    morph = None
    if capability(role) == "morph":
        for node in nodes:
            if node["kind"] == "mesh":
                try:
                    weights = client.call("get_morph_weights", entity=node["entity"])["weights"]
                    if weights:
                        morph = (node["entity"], len(weights))
                        break
                except RuntimeError:
                    continue
        if morph is None:
            raise RuntimeError("Native scene did not expose the model's morph weights")
    started, paused = time.monotonic(), False
    try:
        while True:
            tick = time.monotonic()
            t = min(1.0, (tick-started)/args.duration)
            client.call("set_transform", entity=camera, **orbit_pose(t, center, size))
            if morph and t >= 0.65:
                if not paused:
                    client.call_applied("set_playback", playing=False)
                    wait_for_animation(client, lambda a: not a["playing"])
                    paused = True
                for index in range(morph[1]):
                    weight = (0.5 - 0.5*math.cos((t-0.65)/0.35*math.tau + index))
                    client.call("set_morph_weight", entity=morph[0], index=index, weight=weight)
            if t >= 1:
                break
            time.sleep(max(0, 1/args.fps - (time.monotonic()-tick)))
    finally:
        state = client.call("get_animation")
        if state["playing"]:
            client.call_applied("set_playback", playing=False)
            wait_for_animation(client, lambda a: not a["playing"])
    if morph:
        print(f"  Verified native morph state: {client.call('get_morph_weights', entity=morph[0])}", flush=True)
    if args.screenshot:
        output = args.screenshot.resolve().with_name(args.screenshot.stem + f"-{role}" + args.screenshot.suffix)
        output.parent.mkdir(parents=True, exist_ok=True)
        client.call("screenshot", path=str(output))
        deadline = time.monotonic() + 10
        while not output.exists():
            if time.monotonic() > deadline:
                raise RuntimeError(f"Screenshot did not appear: {output}")
            time.sleep(0.1)
        print(f"  Screenshot: {output}", flush=True)


def run(args):
    if args.screenshot:
        for role in CATALOG:
            output = args.screenshot.with_name(args.screenshot.stem + f"-{role}" + args.screenshot.suffix)
            if output.exists():
                raise RuntimeError(f"Screenshot output already exists: {output}")
    selections, rng = select_models(args.seed, args.asset_cache.resolve(), args.download_timeout)
    print(f"Seed: {args.seed}; remote pools resolved before RNG selection", flush=True)
    if args.dry_run:
        for role, asset in selections:
            print(f"{role}: {asset['name']} ({asset['source']}): {asset['path']}")
        return
    client = Mcp(args.url)
    client.connect()
    for role, asset in selections:
        show_model(client, asset, role, rng, args)
