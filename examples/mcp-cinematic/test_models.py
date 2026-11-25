import copy
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import models


class ModelSelectionTests(unittest.TestCase):
    def setUp(self):
        self.morph = (models.ASSETS / "AnimatedMorphCube.glb").read_bytes()
        self.skinned = (models.ROOT / "upstream/nightshade/assets/gltf/fox.glb").read_bytes()

    def test_remote_resolution_precedes_rng_and_uses_remote_pool(self):
        calls = []
        def fetch(url, timeout):
            calls.append(url)
            return self.morph if "Morph" in url else self.skinned
        with tempfile.TemporaryDirectory() as directory:
            choices, _ = models.select_models(42, Path(directory), fetch=fetch)
            self.assertEqual(len(calls), 5)
            self.assertEqual([(r, a["name"]) for r, a in choices[:2]],
                             [("cube", "AnimatedMorphCube"), ("spring", "CoiledSpring")])
            self.assertTrue(all(asset["source"] == "remote" for role, asset in choices if role != "spring"))
            again, _ = models.select_models(42, Path(directory), fetch=fetch)
            self.assertEqual([(r, a["name"]) for r, a in choices], [(r, a["name"]) for r, a in again])
            with patch.object(models.random, "Random", wraps=models.random.Random) as rng:
                def checked(url, timeout):
                    self.assertFalse(rng.called)
                    return fetch(url, timeout)
                models.select_models(1, Path(directory), fetch=checked)

    def test_failed_or_invalid_remote_uses_valid_local_fallbacks(self):
        def failure(url, timeout):
            raise OSError("offline test")
        with tempfile.TemporaryDirectory() as directory:
            choices, _ = models.select_models(7, Path(directory), fetch=failure)
            self.assertEqual({role for role, _ in choices}, {"cube", "spring", "skeleton"})
            for role, asset in choices:
                self.assertEqual(asset["source"], "local fallback")
                models.parse_glb(asset["path"].read_bytes(), role)
            cached = Path(directory) / "AnimatedMorphCube.glb"
            cached.write_bytes(self.morph)
            pool = models.resolve_pool("morph", Path(directory), fetch=lambda *a: b"not glb")
            self.assertEqual(cached.read_bytes(), self.morph)
            self.assertEqual(pool[0]["source"], "local fallback")

    def test_camera_injection_preserves_original_skeleton_and_animation(self):
        original, _ = models.parse_glb(self.skinned, "skeleton")
        original = copy.deepcopy(original)
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "show.gltf"
            asset = {"path": models.ROOT / "upstream/nightshade/assets/gltf/fox.glb"}
            marker, center, size = models.prepare_scene(asset, "skeleton", output)
            prepared = json.loads(output.read_text())
            self.assertEqual(original["skins"], prepared["skins"])
            self.assertEqual(original["animations"], prepared["animations"])
            self.assertEqual(original["nodes"], prepared["nodes"][:-1])
            self.assertEqual(prepared["nodes"][-1]["name"], marker + " camera")
            self.assertGreater(size, 0)
            self.assertEqual(len(center), 3)

    def test_required_coil_has_a_real_skin_and_two_native_clips(self):
        doc, binary = models.parse_glb((models.ASSETS / "CoiledSpring.glb").read_bytes(), "spring")
        self.assertEqual(len(doc["skins"][0]["joints"]), 13)
        self.assertEqual([a["name"] for a in doc["animations"]], ["Spring compression", "Spring bending"])
        self.assertGreater(len(binary), 10000)

    def test_rejects_static_wrong_role_and_truncated_assets(self):
        with self.assertRaises(ValueError):
            models.parse_glb(self.morph, "skeleton")
        with self.assertRaises(ValueError):
            models.parse_glb(self.skinned, "morph")
        with self.assertRaises(ValueError):
            models.parse_glb(self.morph[:-4], "morph")


if __name__ == "__main__":
    unittest.main()
