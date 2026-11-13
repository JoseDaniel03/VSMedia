"""Offline VSPack build and publishing tests; never build plugin JARs."""
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import sys
import tempfile
import types
import unittest
from unittest.mock import patch
import zipfile

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("vspack_publish", ROOT / "tools" / "vspack_publish.py")
tool = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(tool)


def fixture(files):
    result = io.BytesIO()
    with zipfile.ZipFile(result, "w", zipfile.ZIP_DEFLATED) as archive:
        archive.writestr("pack.mcmeta", '{"pack":{"description":"Virgoland","min_format":[97,1],"max_format":[97,1]}}')
        for path, data in files.items():
            archive.writestr(path, data)
    return result.getvalue()


class VSPackPublishTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.directory = Path(self.temp.name)
        self.source = self.directory / "input.zip"

    def build(self, files, cap=90, profile="lossless"):
        self.source.write_bytes(fixture(files))
        out = self.directory / "output"
        return tool.build(self.source, out, cap, profile, None, False), out

    def test_manifest_deterministic_and_verified(self):
        files = {
            "assets/demo/textures/gui/icon.png": b"\x89PNG" + b"a" * 50,
            "assets/demo/textures/item/sword.png": b"png",
            "assets/demo/lang/es_ar.json": b'{ "hi": "Hola" }'
        }
        report, out = self.build(files)
        self.assertEqual(len(report["packs"]), 3)
        self.assertEqual(report["status"], "built_not_published")
        self.assertTrue(all(p["url"] is None for p in report["packs"]))
        self.assertEqual(tool.verify(out)["packs"], report["packs"])
        for pack in report["packs"]:
            self.assertLess(pack["bytes"], 90 * 1024 * 1024)
            self.assertEqual(len(pack["sha1"]), 40)
            self.assertEqual(len(pack["sha256"]), 64)
            self.assertEqual(len(pack["uuid"]), 36)
            with zipfile.ZipFile(out / pack["filename"]) as item:
                self.assertIn("pack.mcmeta", item.namelist())
        # Same input must emit exactly identical ZIP content/manifest.
        second = self.directory / "again"
        repeated = tool.build(self.source, second, 90, "lossless", None, False)
        self.assertEqual(report, repeated)

    def test_sounds_json_event_is_atomic_when_split(self):
        a = hashlib.shake_256(b"a").digest(710_000)
        b = hashlib.shake_256(b"b").digest(710_000)
        events = {"event_a": {"sounds": ["published/a"]},
                  "event_b": {"sounds": [{"name": "demo:published/b"}]}}
        report, out = self.build({
            "assets/demo/sounds.json": json.dumps(events).encode(),
            "assets/demo/sounds/published/a.ogg": a,
            "assets/demo/sounds/published/b.ogg": b,
        }, cap=1)
        self.assertGreaterEqual(len(report["packs"]), 2)
        seen = set()
        for pack in report["packs"]:
            self.assertLessEqual(pack["bytes"], 1024 * 1024)
            with zipfile.ZipFile(out / pack["filename"]) as archive:
                included = json.loads(archive.read("assets/demo/sounds.json"))
                self.assertEqual(len(included), 1)
                for event in included.values():
                    for reference in event["sounds"]:
                        name = reference if isinstance(reference, str) else reference["name"]
                        path = "assets/demo/sounds/" + name.split(":")[-1] + ".ogg"
                        self.assertIn(path, archive.namelist())
                        seen.add(path)
        self.assertEqual(seen, {"assets/demo/sounds/published/a.ogg",
                                "assets/demo/sounds/published/b.ogg"})

    def test_png_animation_sidecar_stays_with_texture(self):
        report, out = self.build({
            "assets/demo/textures/gui/banner.png": b"test",
            "assets/demo/textures/gui/banner.png.mcmeta": b'{"animation":{"frametime":2}}',
        })
        self.assertEqual(len(report["packs"]), 1)
        with zipfile.ZipFile(out / report["packs"][0]["filename"]) as archive:
            self.assertIn("assets/demo/textures/gui/banner.png", archive.namelist())
            self.assertIn("assets/demo/textures/gui/banner.png.mcmeta", archive.namelist())

    def test_zip_traversal_and_duplicate_rejected(self):
        for path in ("../password.txt", "/etc/passwd", "assets/../hack.png", "assets\\hack.png"):
            with self.subTest(path=path):
                with self.assertRaises(tool.PackError):
                    tool.inspect_zip(fixture({path: b"evil"}))
        with self.assertRaises(tool.PackError):
            tool.inspect_zip(b"not a zip")

    def test_optimizer_fallback_and_strict_option(self):
        self.source.write_bytes(fixture({"assets/demo/lang/es_ar.json": b'{"hi":"hola"}'}))
        report = tool.build(self.source, self.directory / "fallback", 90, "balanced",
                            "/does-not-exist/packsquash", False)
        self.assertTrue(any("no completó" in note for note in report["notes"]))
        with self.assertRaisesRegex(tool.PackError, "obligatorio"):
            tool.build(self.source, self.directory / "strict", 90, "balanced",
                       "/does-not-exist/packsquash", True)

    def test_tampered_zip_does_not_pass_verification(self):
        report, out = self.build({"assets/demo/lang/en_us.json": b'{}'})
        (out / report["packs"][0]["filename"]).write_bytes(b"changed")
        with self.assertRaises(tool.PackError):
            tool.verify(out)

    def test_r2_publishes_immutable_packs_then_current_pointer(self):
        report, out = self.build({"assets/demo/lang/en_us.json": b'{}'})
        uploaded = []
        class Client:
            def put_object(self, **kwargs):
                uploaded.append(kwargs)
        fake = types.SimpleNamespace(client=lambda *args, **kwargs: Client())
        env = {
            "VSPACK_R2_ENDPOINT": "https://example.r2.cloudflarestorage.com",
            "VSPACK_R2_BUCKET": "packs",
            "VSPACK_R2_PUBLIC_URL": "https://packs.example.org",
            "VSPACK_R2_ACCESS_KEY_ID": "dummy-for-test",
            "VSPACK_R2_SECRET_ACCESS_KEY": "dummy-for-test",
        }
        with patch.dict(os.environ, env), patch.dict(sys.modules, {"boto3": fake}):
            published = tool.publish_r2(out, "virgoland")
        self.assertEqual(len(uploaded), len(report["packs"]) + 1)
        self.assertTrue(all(x["Key"].startswith("virgoland/versions/")
                            for x in uploaded[:-1]))
        self.assertEqual(uploaded[-1]["Key"], "virgoland/current.json")
        self.assertEqual(uploaded[-1]["ContentType"], "application/json")
        self.assertEqual(published["status"], "uploaded_to_r2")
        self.assertTrue(published["packs"][0]["url"].startswith("https://packs.example.org/"))
        self.assertNotIn("dummy-for-test", (out / "published-manifest.json").read_text())

    def test_r2_requires_private_credentials(self):
        report, out = self.build({"assets/demo/lang/en_us.json": b'{}'})
        with patch.dict(os.environ, {
            "VSPACK_R2_ENDPOINT": "", "VSPACK_R2_PUBLIC_URL": "",
            "VSPACK_R2_ACCESS_KEY_ID": "", "VSPACK_R2_SECRET_ACCESS_KEY": "",
        }):
            with self.assertRaises(tool.PackError):
                tool.publish_r2(out, "virgoland")


    def test_studio_source_rejects_remote_urls_and_redirect_tricks(self):
        for value in ("https://example.org/demo.zip",
                      "http://127.0.0.1:3000/api/v1/packs/export",
                      "http://evil.example:3061/api/v1/packs/export",
                      "http://127.0.0.1:3061/api/v1/packs/export?target=evil",
                      "http://127.0.0.1:3061/anything-else"):
            with self.subTest(value=value):
                with self.assertRaises(tool.PackError):
                    tool.download_studio_export(value)

    def test_r2_rollback_updates_only_current_pointer(self):
        report, out = self.build({"assets/demo/lang/en_us.json": b'{}'})
        uploaded = []
        class Client:
            def put_object(self, **kwargs):
                uploaded.append(kwargs)
        fake = types.SimpleNamespace(client=lambda *args, **kwargs: Client())
        env = {
            "VSPACK_R2_ENDPOINT": "https://example.r2.cloudflarestorage.com",
            "VSPACK_R2_BUCKET": "packs",
            "VSPACK_R2_PUBLIC_URL": "https://packs.example.org",
            "VSPACK_R2_ACCESS_KEY_ID": "fake",
            "VSPACK_R2_SECRET_ACCESS_KEY": "fake",
        }
        with patch.dict(os.environ, env), patch.dict(sys.modules, {"boto3": fake}):
            tool.publish_r2(out, "virgoland")
            uploaded.clear()
            tool.rollback_r2(out, "virgoland")
        self.assertEqual(len(uploaded), 1)
        self.assertEqual(uploaded[0]["Key"], "virgoland/current.json")
        self.assertEqual(uploaded[0]["ContentType"], "application/json")


    def test_watch_builds_only_changed_source(self):
        payload = fixture({"assets/demo/lang/en_us.json": b'{"hello":"world"}'})
        root = self.directory / "watch"
        with patch.object(tool, "download_studio_export", return_value=payload):
            first = tool.watch_once(root, "http://127.0.0.1:3061/api/v1/packs/export",
                                    90, "lossless", None, False, False, "virgoland")
            second = tool.watch_once(root, "http://127.0.0.1:3061/api/v1/packs/export",
                                     90, "lossless", None, False, False, "virgoland")
        self.assertTrue(first["changed"])
        self.assertFalse(second["changed"])
        self.assertEqual(first["source_sha256"], second["source_sha256"])
        self.assertTrue((root / first["revision"] / "manifest.json").is_file())
        self.assertEqual(len(list(root.glob("revision-*"))), 1)

    def test_watch_failed_publish_does_not_promote_state(self):
        payload = fixture({"assets/demo/lang/en_us.json": b'{}'})
        root = self.directory / "failedwatch"
        with patch.object(tool, "download_studio_export", return_value=payload):
            with patch.object(tool, "publish_r2", side_effect=tool.PackError("R2 no disponible")):
                with self.assertRaises(tool.PackError):
                    tool.watch_once(root, "http://127.0.0.1:3061/api/v1/packs/export",
                                    90, "lossless", None, False, True, "virgoland")
        self.assertFalse((root / "publication-state.json").exists())



    def test_watch_blocks_missing_session_sources_after_restart(self):
        payload = fixture({"assets/demo/lang/en_us.json": b'{}'})
        root = self.directory / "sources"
        with patch.object(tool, "download_studio_export", return_value=payload):
            with patch.object(tool, "studio_session_pack_count", return_value=2):
                tool.watch_once(root, "http://127.0.0.1:3061/api/v1/packs/export",
                                90, "lossless", None, False, False, "virgoland")
            with patch.object(tool, "studio_session_pack_count", return_value=0):
                with self.assertRaisesRegex(tool.PackError, "ya no están cargados"):
                    tool.watch_once(root, "http://127.0.0.1:3061/api/v1/packs/export",
                                    90, "lossless", None, False, False, "virgoland")


    def test_watch_rebuilds_when_compression_settings_change(self):
        payload = fixture({"assets/demo/lang/es_ar.json": b'{"radio":"ok"}'})
        root = self.directory / "settings"
        with patch.object(tool, "download_studio_export", return_value=payload):
            first = tool.watch_once(root, "http://127.0.0.1:3061/api/v1/packs/export",
                                    90, "lossless", None, False, False, "virgoland")
            changed = tool.watch_once(root, "http://127.0.0.1:3061/api/v1/packs/export",
                                      95, "lossless", None, False, False, "virgoland")
            same = tool.watch_once(root, "http://127.0.0.1:3061/api/v1/packs/export",
                                   95, "lossless", None, False, False, "virgoland")
        self.assertTrue(first["changed"])
        self.assertTrue(changed["changed"])
        self.assertFalse(same["changed"])
        self.assertNotEqual(first["revision"], changed["revision"])
        self.assertEqual(len(list(root.glob("revision-*"))), 2)

    def test_watch_republishes_when_cdn_target_changes(self):
        payload = fixture({"assets/demo/lang/es_ar.json": b'{}'})
        root = self.directory / "cdn-target"
        def fake_publish(folder, prefix):
            tool.verify(folder)
            return {"status": "uploaded_to_r2"}
        with patch.object(tool, "download_studio_export", return_value=payload), \
             patch.object(tool, "publish_r2", side_effect=fake_publish) as uploader:
            first = tool.watch_once(root, "http://127.0.0.1:3061/api/v1/packs/export",
                                    90, "lossless", None, False, True, "virgoland")
            cached = tool.watch_once(root, "http://127.0.0.1:3061/api/v1/packs/export",
                                     90, "lossless", None, False, True, "virgoland")
            changed = tool.watch_once(root, "http://127.0.0.1:3061/api/v1/packs/export",
                                      90, "lossless", None, False, True, "another")
        self.assertTrue(first["changed"])
        self.assertFalse(cached["changed"])
        self.assertTrue(changed["changed"])
        self.assertEqual(uploader.call_count, 2)

    def test_r2_reuses_verified_immutable_objects_without_uploading_again(self):
        report, out = self.build({"assets/demo/lang/en_us.json": b'{"message":"abc"}'})
        cloud = {}
        writes = []
        class MissingObject(Exception):
            def __init__(self):
                self.response = {"Error": {"Code": "404"}}
        class Client:
            def head_object(self, **kwargs):
                if kwargs["Key"] not in cloud:
                    raise MissingObject()
                body, metadata = cloud[kwargs["Key"]]
                return {"ContentLength": len(body), "Metadata": metadata}
            def put_object(self, **kwargs):
                body = kwargs["Body"]
                if hasattr(body, "read"):
                    if isinstance(body, bytes):
                        raise AssertionError("Expected streaming body instead of in-memory bytes")
                    body = body.read()
                cloud[kwargs["Key"]] = (body, kwargs.get("Metadata", {}))
                writes.append(kwargs["Key"])
        client = Client()
        # This fake only reads from the source; real boto3 streams it.
        fake = types.SimpleNamespace(client=lambda *args, **kwargs: client)
        env = {"VSPACK_R2_ENDPOINT":"https://example.r2.cloudflarestorage.com",
               "VSPACK_R2_BUCKET":"packs", "VSPACK_R2_PUBLIC_URL":"https://packs.example.org",
               "VSPACK_R2_ACCESS_KEY_ID":"test", "VSPACK_R2_SECRET_ACCESS_KEY":"test"}
        with patch.dict(os.environ, env), patch.dict(sys.modules, {"boto3": fake}):
            tool.publish_r2(out, "virgoland")
            tool.publish_r2(out, "virgoland")
        keys = [x for x in writes if "/versions/" in x]
        self.assertEqual(len(keys), len(report["packs"]))
        self.assertEqual(writes[-1], "virgoland/current.json")
        self.assertEqual(writes.count("virgoland/current.json"), 2)

    def test_r2_collision_never_promotes_manifest(self):
        report, out = self.build({"assets/demo/lang/en_us.json": b'{}'})
        writes = []
        class Client:
            def head_object(self, **kwargs):
                return {"ContentLength": 1, "Metadata": {"sha256":"other", "sha1":"bad"}}
            def put_object(self, **kwargs):
                writes.append(kwargs["Key"])
        fake = types.SimpleNamespace(client=lambda *args, **kwargs: Client())
        env = {"VSPACK_R2_ENDPOINT":"https://example.r2.cloudflarestorage.com",
               "VSPACK_R2_BUCKET":"packs", "VSPACK_R2_PUBLIC_URL":"https://packs.example.org",
               "VSPACK_R2_ACCESS_KEY_ID":"test", "VSPACK_R2_SECRET_ACCESS_KEY":"test"}
        with patch.dict(os.environ, env), patch.dict(sys.modules, {"boto3": fake}):
            with self.assertRaisesRegex(tool.PackError, "Colisión"):
                tool.publish_r2(out, "virgoland")
        self.assertEqual(writes, [])

    def test_watch_verifies_cached_zip_before_skipping(self):
        payload = fixture({"assets/demo/lang/en_us.json": b'{}'})
        root = self.directory / "corrupt"
        with patch.object(tool, "download_studio_export", return_value=payload):
            first = tool.watch_once(root, "http://127.0.0.1:3061/api/v1/packs/export",
                                    90, "lossless", None, False, False, "virgoland")
            rev=root / first["revision"]
            manifest=tool.verify(rev)
            (rev / manifest["packs"][0]["filename"]).write_bytes(b"tampered")
            with self.assertRaises(tool.PackError):
                tool.watch_once(root, "http://127.0.0.1:3061/api/v1/packs/export",
                                90, "lossless", None, False, False, "virgoland")


if __name__ == "__main__":
    unittest.main()
