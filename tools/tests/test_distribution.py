import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location('build_dist', ROOT / 'retro-go-sd/scripts/build_dist.py')
build_dist = importlib.util.module_from_spec(spec)
spec.loader.exec_module(build_dist)


class DistributionTests(unittest.TestCase):
    def test_streaming_mapped_output_is_retained(self):
        manifest = {
            'targets': [{
                'kind': 'homebrew', 'requiresAbi': {'version': 2, 'minSize': 888},
                'artifacts': [{'filename': 'code.xip', 'mapped': True, 'relocations': {}}],
                'uses': [{'tool': 'assets', 'required': True}],
            }],
            'tools': [{
                'id': 'assets', 'inputs': [{'required': True, 'maxBytes': 4699979776}],
                'outputs': [{'id': 'assets', 'filename': 'assets.xip', 'mapped': True, 'relocations': {}}],
            }],
        }
        self.assertIsNone(build_dist.manifest_problem(manifest))
        release = {'publishedAt': '2026-11-01T00:00:00Z', 'isPrerelease': False}
        entry = build_dist.index_entry('v0.0.1', release, manifest, 'smg-v0.0.1-bundle.zip')
        self.assertTrue(entry['needsUserFiles'])
        self.assertEqual(entry['manifest'], 'v0.0.1/manifest.json')
        self.assertEqual(entry['bundle'], 'smg-v0.0.1-bundle.zip')

    def test_unknown_output_fields_are_not_silently_published(self):
        manifest = {'tools': [{'outputs': [{'id': 'out', 'unknown': True}]}]}
        self.assertIn('unknown', build_dist.manifest_problem(manifest))

    def test_release_mirror_contains_index_manifest_and_bundle(self):
        manifest = {
            'schemaVersion': 1, 'project': 'smg', 'title': 'SMG',
            'source': {'ref': 'v0.0.1'},
            'targets': [{'kind': 'homebrew', 'requiresAbi': {'version': 2, 'minSize': 888},
                         'artifacts': [{'filename': 'SMG.bin', 'url': 'SMG.bin'}]}],
            'tools': [],
        }
        release = {'tagName': 'v0.0.1', 'publishedAt': '2026-11-01T00:00:00Z', 'isPrerelease': False}

        def fetch(repo, tag, name, dest, **kwargs):
            if name == 'manifest.json':
                (dest / name).write_text(json.dumps(manifest))
            elif name in ('SMG.bin', 'smg-v0.0.1-bundle.zip'):
                (dest / name).write_bytes(b'fixture')
            else:
                return False
            return True

        with tempfile.TemporaryDirectory() as directory, \
             patch.object(build_dist, 'list_releases', return_value=[release]), \
             patch.object(build_dist, 'fetch_asset', side_effect=fetch):
            out = Path(directory) / 'dist'
            build_dist.build(repo='owner/repo', out=out, retain=3, require_tag='v0.0.1')
            self.assertTrue((out / 'v0.0.1/SMG.bin').is_file())
            self.assertTrue((out / 'smg-v0.0.1-bundle.zip').is_file())
            index = json.loads((out / 'versions.json').read_text())
            self.assertEqual(index['versions'][0]['manifest'], 'v0.0.1/manifest.json')


if __name__ == '__main__':
    unittest.main()
