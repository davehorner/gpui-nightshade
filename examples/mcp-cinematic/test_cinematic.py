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
