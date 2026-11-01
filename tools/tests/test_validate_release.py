import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
import zipfile

ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / 'retro-go-sd/scripts/validate_release.py'


class ReleaseValidationTests(unittest.TestCase):
    def test_release_bundle_must_match_tag_commit_and_hash(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            data = b'fixture'
            manifest = {
                'source': {'repo': 'slash-proc/smg-retro-go-sd', 'ref': 'v0.0.1', 'commit': 'abc'},
                'tools': [], 'targets': [{'artifacts': [{
                    'url': 'SMG.bin', 'bytes': len(data), 'sha256': hashlib.sha256(data).hexdigest(),
                }]}],
            }
            bundle = root / 'bundle.zip'
            with zipfile.ZipFile(bundle, 'w') as archive:
                archive.writestr('manifest.json', json.dumps(manifest))
                archive.writestr('SMG.bin', data)
            def run(commit, output):
                return subprocess.run([sys.executable, str(SCRIPT), '--bundle', str(bundle), '--tag', 'v0.0.1',
                                       '--commit', commit, '--out', str(root / output)], capture_output=True)
            self.assertEqual(run('abc', 'valid').returncode, 0)
            self.assertEqual((root / 'valid/SMG.bin').read_bytes(), data)
            self.assertNotEqual(run('wrong', 'bad').returncode, 0)
            with zipfile.ZipFile(bundle, 'w') as archive:
                archive.writestr('manifest.json', json.dumps(manifest))
                archive.writestr('SMG.bin', b'corrupt')
            self.assertNotEqual(run('abc', 'bad-hash').returncode, 0)


if __name__ == '__main__':
    unittest.main()
