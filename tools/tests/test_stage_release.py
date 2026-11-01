import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
import zipfile

ROOT = Path(__file__).resolve().parents[2]


class StageReleaseTests(unittest.TestCase):
    def test_staging_before_tag_pushes_bundle_to_release_input_branch(self):
        with tempfile.TemporaryDirectory(prefix='stage-release-test-') as directory:
            root = Path(directory)
            remote, repo, fake = root / 'remote.git', root / 'repo', root / 'fake'
            fake.mkdir()
            def run(*args, cwd=None, env=None):
                return subprocess.run(args, cwd=cwd, env=env, check=True, capture_output=True)
            run('git', 'init', '--bare', str(remote))
            run('git', 'clone', str(remote), str(repo))
            run('git', 'config', 'user.name', 'Test', cwd=repo)
            run('git', 'config', 'user.email', 'test@example.invalid', cwd=repo)
            scripts = repo / 'retro-go-sd/scripts'
            scripts.mkdir(parents=True)
            for name in ('stage_release.sh', 'validate_release.py', 'make_bundle.py'):
                shutil.copy(ROOT / 'retro-go-sd/scripts' / name, scripts / name)
            run('git', 'add', '.', cwd=repo)
            run('git', 'commit', '-m', 'fixture', cwd=repo)
            run('git', 'branch', '-M', 'main', cwd=repo)
            run('git', 'push', 'origin', 'main', cwd=repo)
            commit = run('git', 'rev-parse', 'HEAD', cwd=repo).stdout.decode().strip()
            files = repo / 'candidate'
            files.mkdir()
            data = b'x'
            manifest = {
                'source': {'repo': 'slash-proc/smg-retro-go-sd', 'ref': 'v0.0.2', 'commit': commit},
                'tools': [], 'targets': [{'artifacts': [{
                    'url': 'smg_asset_converter.wasm', 'bytes': 1,
                    'sha256': hashlib.sha256(data).hexdigest(),
                }]}],
            }
            (files / 'manifest.json').write_text(json.dumps(manifest))
            (files / 'smg_iso_verifier.wasm').write_bytes(data)
            with zipfile.ZipFile(files / 'smg-v0.0.2-bundle.zip', 'w') as archive:
                archive.writestr('manifest.json', json.dumps(manifest))
                archive.writestr('smg_asset_converter.wasm', data)
            # The separate verifier tests exercise real WASM; this test isolates
            # Git branch creation and the pre-tag staging path.
            node = fake / 'node'
            node.write_text('#!/bin/sh\nexit 0\n')
            node.chmod(0o755)
            env = dict(os.environ, PATH=str(fake) + os.pathsep + os.environ['PATH'])
            run('sh', 'retro-go-sd/scripts/stage_release.sh', 'v0.0.2', str(files), cwd=repo, env=env)
            run('git', '--git-dir', str(remote), 'rev-parse', 'refs/heads/release-input/v0.0.2')
            self.assertNotEqual(run('git', 'tag', '-l', 'v0.0.2', cwd=repo).stdout, b'v0.0.2\n')


if __name__ == '__main__':
    unittest.main()
