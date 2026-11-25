import base64
import struct
import json
import math
from pathlib import Path
import tempfile
import unittest

import cinematic as demo


class ChoreographyTests(unittest.TestCase):
    def test_camera_aims_at_subject_throughout_path(self):
        for i in range(1001):
            t = i / 1000
            pose = demo.camera_pose(t)
            pitch, yaw = map(math.radians, pose["rotation"][:2])
            forward = [-math.sin(yaw) * math.cos(pitch), math.sin(pitch),
                       -math.cos(yaw) * math.cos(pitch)]
            target = [0, 1.4 + 0.8 * math.sin(math.pi * t), 0]
            delta = [b - a for a, b in zip(pose["translation"], target)]
            length = math.sqrt(sum(x * x for x in delta))
            self.assertGreater(sum(a * b / length for a, b in zip(forward, delta)), 0.999999)

    def test_meshes_have_outward_normals(self):
        for shape in ("box", "diamond", "pyramid"):
            positions, normals = demo.geometry(shape)
            for i in range(0, len(positions), 3):
                self.assertGreater(sum(positions[i + j] * normals[i + j] for j in range(3)), 0)

    def test_generated_clip_targets_objects_and_camera_and_has_real_morph_deltas(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "animated.gltf"
            demo.write_scene(path, demo.choreography(7, 3), duration=2)
            doc = json.loads(path.read_text())
            blob = base64.b64decode(doc["buffers"][0]["uri"].split(",")[1])
            def values(index):
                accessor = doc["accessors"][index]
                view = doc["bufferViews"][accessor["bufferView"]]
                return struct.unpack_from("<" + "f" * (view["byteLength"] // 4), blob, view["byteOffset"])
            clip = doc["animations"][0]
            targets = {(c["target"]["node"], c["target"]["path"]) for c in clip["channels"]}
            for node in range(3):
                for field in ("translation", "rotation", "scale"):
                    self.assertIn((node, field), targets)
            self.assertIn((3, "translation"), targets)
            self.assertIn((3, "rotation"), targets)
            for sampler in clip["samplers"]:
                times = values(sampler["input"])
                self.assertEqual((times[0], times[-1]), (0, 2))
                self.assertTrue(all(a < b for a, b in zip(times, times[1:])))
                self.assertEqual(doc["accessors"][sampler["output"]]["count"], len(times))
            for channel in clip["channels"]:
                if channel["target"]["path"] == "rotation":
                    data = values(clip["samplers"][channel["sampler"]]["output"])
                    for i in range(0, len(data), 4):
                        self.assertAlmostEqual(sum(v * v for v in data[i:i+4]), 1, places=5)
            primitive = doc["meshes"][0]["primitives"][0]
            self.assertEqual(doc["meshes"][0]["weights"], [0, 0])
            for target in primitive["targets"]:
                delta = values(target["POSITION"])
                self.assertTrue(any(abs(v) > 0.1 for v in delta))
                self.assertEqual(len(delta), len(values(primitive["attributes"]["POSITION"])))

    def test_seeded_scene_and_tweens_have_stable_endpoints(self):
        objects = demo.choreography(7, 6)
        self.assertEqual(objects, demo.choreography(7, 6))
        self.assertNotEqual(objects, demo.choreography(8, 6))
        for i, obj in enumerate(objects):
            for t in (0, 1):
                pose = demo.object_pose(obj, i, len(objects), t)
                self.assertEqual(pose["translation"], obj["home"])
                self.assertEqual(pose["scale"], [obj["size"] * v for v in obj["proportions"]])
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "scene.gltf"
            demo.write_scene(path, objects)
            doc = json.loads(path.read_text())
            self.assertEqual(len(doc["materials"]), 1)
            self.assertEqual(len(doc["nodes"]), 7)
            self.assertEqual(doc["nodes"][-1]["camera"], 0)
            for accessor in doc["accessors"]:
                view = doc["bufferViews"][accessor["bufferView"]]
                self.assertLessEqual(view["byteOffset"] + view["byteLength"],
                                     doc["buffers"][0]["byteLength"])


if __name__ == "__main__":
    unittest.main()
