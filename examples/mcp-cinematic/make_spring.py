"""Build the included coiled spring: actual helix geometry, skin and two clips."""

import json
import math
from pathlib import Path
import struct


def build(path):
    turns, steps, sides, joints = 6, 240, 10, 13
    height, radius, wire = 3.0, 0.65, 0.09
    positions, normals, joint_ids, weights, indices = [], [], [], [], []
    for ring in range(steps + 1):
        u = ring / steps
        angle = u * turns * math.tau
        radial = [math.cos(angle), 0, math.sin(angle)]
        tangent = [-radius * math.sin(angle), height / (turns * math.tau), radius * math.cos(angle)]
        side = [radial[1]*tangent[2]-radial[2]*tangent[1],
                radial[2]*tangent[0]-radial[0]*tangent[2],
                radial[0]*tangent[1]-radial[1]*tangent[0]]
        length = math.sqrt(sum(v*v for v in side))
        side = [v / length for v in side]
        center = [radius * math.cos(angle), u * height, radius * math.sin(angle)]
        at = u * (joints - 1)
        low = min(int(at), joints - 2)
        blend = at - low
        for edge in range(sides):
            phi = edge / sides * math.tau
            normal = [math.cos(phi)*a + math.sin(phi)*b for a, b in zip(radial, side)]
            positions.extend(c + wire*n for c, n in zip(center, normal))
            normals.extend(normal)
            joint_ids.extend([low, low + 1, 0, 0])
            weights.extend([1-blend, blend, 0, 0])
    for ring in range(steps):
        for edge in range(sides):
            a, b = ring*sides+edge, ring*sides+(edge+1) % sides
            c, d = a+sides, b+sides
            indices.extend([a, c, b, b, c, d])
    # Match winding to the outward analytic normal.
    a, b, c = [positions[i*3:i*3+3] for i in indices[:3]]
    u, v = [[y-x for x, y in zip(a, p)] for p in (b, c)]
    cross = [u[1]*v[2]-u[2]*v[1], u[2]*v[0]-u[0]*v[2], u[0]*v[1]-u[1]*v[0]]
    if sum(a*b for a, b in zip(cross, normals[:3])) < 0:
        for i in range(0, len(indices), 3):
            indices[i+1], indices[i+2] = indices[i+2], indices[i+1]
    blob = bytearray()
    doc = {"asset": {"version": "2.0", "generator": "gpui-nightshade make_spring.py"},
           "scene": 0, "scenes": [{"nodes": [0]}], "nodes": [], "meshes": [],
           "bufferViews": [], "accessors": [], "skins": [], "animations": [],
           "materials": [{"name": "Spring brass", "pbrMetallicRoughness": {
               "baseColorFactor": [0.75, 0.38, 0.08, 1], "metallicFactor": 0.65, "roughnessFactor": 0.3}}]}
    def attribute(values, kind, component=5126, target=None):
        width = {"SCALAR": 1, "VEC3": 3, "VEC4": 4, "MAT4": 16}[kind]
        blob.extend(b"\0" * (-len(blob) % 4))
        view = {"buffer": 0, "byteOffset": len(blob)}
        fmt = {5126: "f", 5123: "H", 5125: "I"}[component]
        packed = struct.pack("<" + fmt * len(values), *values)
        blob.extend(packed)
        view["byteLength"] = len(packed)
        if target:
            view["target"] = target
        index = len(doc["accessors"])
        doc["bufferViews"].append(view)
        doc["accessors"].append({"bufferView": len(doc["bufferViews"])-1,
            "componentType": component, "type": kind, "count": len(values)//width,
            **({"min": [min(values[i::width]) for i in range(width)],
                "max": [max(values[i::width]) for i in range(width)]} if kind != "MAT4" else {})})
        return index
    primitive = {"attributes": {"POSITION": attribute(positions, "VEC3", target=34962),
        "NORMAL": attribute(normals, "VEC3", target=34962),
        "JOINTS_0": attribute(joint_ids, "VEC4", 5123, 34962),
        "WEIGHTS_0": attribute(weights, "VEC4", target=34962)},
        "indices": attribute(indices, "SCALAR", 5125, 34963), "material": 0}
    doc["meshes"] = [{"name": "Coiled spring", "primitives": [primitive]}]
    doc["nodes"] = [{"name": "Spring assembly", "children": [1, 2]},
                    {"name": "Coiled spring", "mesh": 0, "skin": 0}]
    inverse = []
    for i in range(joints):
        node = {"name": f"Spring joint {i:02d}", "translation": [0, height/(joints-1) if i else 0, 0]}
        if i < joints-1:
            node["children"] = [i+3]
        doc["nodes"].append(node)
        matrix = [float(k % 5 == 0) for k in range(16)]
        matrix[13] = -i * height/(joints-1)
        inverse.extend(matrix)
    doc["skins"] = [{"name": "Spring skeleton", "joints": list(range(2, joints+2)),
                     "skeleton": 2, "inverseBindMatrices": attribute(inverse, "MAT4")}]
    times = attribute([0, 1, 2, 3, 4], "SCALAR")
    scales = [1, 1, 1, 1, 0.55, 1, 1, 1, 1, 1, 1.15, 1, 1, 1, 1]
    doc["animations"].append({"name": "Spring compression", "samplers": [
        {"input": times, "output": attribute(scales, "VEC3"), "interpolation": "LINEAR"}],
        "channels": [{"sampler": 0, "target": {"node": 2, "path": "scale"}}]})
    bend = {"name": "Spring bending", "samplers": [], "channels": []}
    for i in range(joints):
        rotations = []
        for angle in [0, 0.045, 0, -0.045, 0]:
            rotations.extend([0, 0, math.sin(angle/2), math.cos(angle/2)])
        bend["samplers"].append({"input": times, "output": attribute(rotations, "VEC4"), "interpolation": "LINEAR"})
        bend["channels"].append({"sampler": i, "target": {"node": i+2, "path": "rotation"}})
    doc["animations"].append(bend)
    blob.extend(b"\0" * (-len(blob) % 4))
    doc["buffers"] = [{"byteLength": len(blob)}]
    data = json.dumps(doc).encode()
    data += b" " * (-len(data) % 4)
    path.write_bytes(struct.pack("<III", 0x46546C67, 2, 28+len(data)+len(blob))
                     + struct.pack("<II", len(data), 0x4E4F534A) + data
                     + struct.pack("<II", len(blob), 0x004E4942) + blob)


if __name__ == "__main__":
    build(Path(__file__).resolve().parent / "assets/CoiledSpring.glb")
