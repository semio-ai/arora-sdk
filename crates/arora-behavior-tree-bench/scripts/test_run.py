"""Exercise orchestration with local stand-ins; no Git repository is modified."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest


class RunnerTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.bin = self.root / 'bin'
        self.bin.mkdir()
        self.log = self.root / 'commands.jsonl'
        self.out = self.root / 'out'
        self.env = {**os.environ, 'PATH': str(self.bin) + os.pathsep + os.environ['PATH'],
                    'FAKE_LOG': str(self.log), 'FAKE_ROOT': str(Path(__file__).resolve().parents[3]),
                    'BENCH_CARGO_FLAGS': '--offline --quiet'}
        self.env.pop('ROUNDS', None)
        self.tool('git', '''
import shutil
if args == ['rev-parse', '--show-toplevel']:
    print(os.environ['FAKE_ROOT'])
elif 'rev-parse' in args:
    print('a' * 40)
elif 'add' in args:
    Path(args[-2]).mkdir(parents=True)
elif 'remove' in args:
    shutil.rmtree(args[-1], ignore_errors=True)
else:
    sys.exit('unexpected git invocation')
''')
        self.tool('cargo', '''
if os.environ.get('FAIL_BUILD'):
    sys.exit(9)
p = Path(os.environ['CARGO_TARGET_DIR']) / 'release' / 'bench-kit'
p.parent.mkdir(parents=True)
p.write_text(r"""#!/usr/bin/env python3
import json, os, sys
from pathlib import Path
with open(os.environ['FAKE_LOG'], 'a') as out:
    out.write(json.dumps(['round', sys.argv[1]]) + '\\n')
entry = {'times': [[1]], 'written': ['0123456789abcdef'], 'differing': 0}
record = {'schema': 1, 'platform': 'test', 'warmUp': 0, 'ticks': 1, 'dt': .1, 'phases': ['one'], 'forms': {'detached': entry}}
Path(sys.argv[1]).write_text(json.dumps({'schema': 1, 'tests': {'synthetic': {'kind': 'workload', 'size': 0, 'record': record}}}))
""")
p.chmod(0o755)
''')

    def tool(self, name, body):
        path = self.bin / name
        path.write_text('''#!/usr/bin/env python3
import json, os, sys
from pathlib import Path
args = sys.argv[1:]
with open(os.environ['FAKE_LOG'], 'a') as log:
    log.write(json.dumps([Path(sys.argv[0]).name, *args]) + '\\n')
''' + body)
        path.chmod(0o755)

    def run_script(self, **env):
        return subprocess.run(['bash', str(Path(__file__).with_name('run.sh')), str(self.out),
                               'a=HEAD', 'b=HEAD'], env={**self.env, **env}, capture_output=True, text=True)

    def commands(self):
        return [json.loads(line) for line in self.log.read_text().splitlines()]

    def test_eight_balanced_rounds_extra_flags_and_cleanup(self):
        result = self.run_script()
        self.assertEqual(result.returncode, 0, result.stderr)
        manifest = json.loads((self.out / 'manifest.json').read_text())
        self.assertEqual(manifest['rounds'], 8)
        commands = self.commands()
        builds = [c for c in commands if c[0] == 'cargo']
        self.assertEqual(len(builds), 2)
        for build in builds:
            self.assertIn('--locked', build)
            self.assertIn('--offline', build)
            self.assertIn('--quiet', build)
        order = [Path(c[1]).parent.name for c in commands if c[0] == 'round']
        self.assertEqual(order, ['a', 'b', 'b', 'a'] * 4)
        self.assertFalse(any((self.out / 'worktrees').iterdir()))
        self.assertEqual(len([c for c in commands if 'remove' in c]), 2)

    def test_default_build_allows_network(self):
        result = self.run_script(BENCH_CARGO_FLAGS='')
        self.assertEqual(result.returncode, 0, result.stderr)
        for command in self.commands():
            if command[0] == 'cargo':
                self.assertIn('--locked', command)
                self.assertNotIn('--offline', command)

    def test_build_failure_removes_created_worktrees(self):
        result = self.run_script(FAIL_BUILD='1')
        self.assertEqual(result.returncode, 9, result.stderr)
        self.assertFalse(any((self.out / 'worktrees').iterdir()))
        self.assertEqual(len([c for c in self.commands() if 'remove' in c]), 1)

    def test_too_few_and_odd_rounds_fail_before_build(self):
        for rounds in ['1', '3', '5']:
            result = self.run_script(ROUNDS=rounds)
            self.assertEqual(result.returncode, 2)
            self.assertIn('even and at least 4', result.stderr)
        self.assertFalse(any(c[0] == 'cargo' for c in self.commands()))


if __name__ == '__main__':
    unittest.main()
