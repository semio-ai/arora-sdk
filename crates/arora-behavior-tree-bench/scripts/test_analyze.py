"""Protocol tests with independently constructed round data."""
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('analyze', Path(__file__).with_name('analyze.py'))
analyze = importlib.util.module_from_spec(spec)
spec.loader.exec_module(analyze)


class ComparisonTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.manifest = {'schema': 1, 'rounds': 3, 'builds': {'base': 'a', 'second': 'b', 'third': 'c'}}
        (self.root / 'manifest.json').write_text(json.dumps(self.manifest))
        for build, ratio in [('base', 1), ('second', 2), ('third', 0.5)]:
            folder = self.root / 'runs' / build
            folder.mkdir(parents=True)
            for round_ in range(1, 4):
                tests = {}
                for name, kind, size, factor in [('work', 'workload', 0, 1), ('chain/10', 'scale', 10, 1), ('chain/30', 'scale', 30, 4)]:
                    forms = {}
                    for form in ['detached', 'attached-naive']:
                        scale = ratio if name == 'work' else ratio * factor if build != 'base' else 1
                        forms[form] = {'times': [[100, round_ * scale, round_ * scale]], 'written': ['0123456789abcdef'] * 3, 'differing': 0}
                    tests[name] = {'kind': kind, 'size': size, 'record': {'schema': 1, 'platform': 'test', 'warmUp': 1, 'ticks': 3, 'dt': 0.1, 'phases': ['one'], 'forms': forms}}
                self.put(build, round_, {'schema': 1, 'tests': tests})

    def path(self, build='second', round_=1):
        return self.root / 'runs' / build / f'{round_}.json'

    def put(self, build, round_, data):
        self.path(build, round_).write_text(json.dumps(data))

    def mutate(self, change):
        data = json.loads(self.path().read_text())
        change(data)
        self.put('second', 1, data)

    def test_ratios_size_geomeans_subsets_and_fixed_bootstrap(self):
        result, _ = analyze.compare(self.root, 'base')
        self.assertEqual(result['resamples'], 2000)
        self.assertEqual(result['agreementCells'], 54)
        self.assertEqual(result['tests']['work']['second/detached']['ratio'], 2)
        self.assertAlmostEqual(result['tests']['chain']['second/detached']['ratio'], 4)
        self.assertAlmostEqual(result['tests']['overall']['second/detached']['ratio'], 8 ** 0.5)
        self.assertAlmostEqual(result['tests']['workloads']['third/detached']['ratio'], 0.5)
        self.assertAlmostEqual(result['tests']['scenarios']['third/detached']['ratio'], 1)
        low, high = result['tests']['work']['second/detached']['ci']
        self.assertLess(low, 2)
        self.assertGreater(high, 2)
        self.assertEqual(result, analyze.compare(self.root, 'base')[0])

    def test_chosen_form(self):
        result, _ = analyze.compare(self.root, 'base', ['detached'])
        self.assertEqual(result['forms'], ['detached'])
        self.assertEqual(result['agreementCells'], 27)

    def test_missing_round(self):
        self.path().unlink()
        with self.assertRaisesRegex(ValueError, 'round files'):
            analyze.compare(self.root, 'base')

    def test_missing_form(self):
        self.mutate(lambda d: d['tests']['work']['record']['forms'].pop('detached'))
        with self.assertRaisesRegex(ValueError, 'missing or duplicate forms'):
            analyze.compare(self.root, 'base')

    def test_digest_mismatch_is_rejected(self):
        def change(d):
            for form in d['tests']['work']['record']['forms'].values():
                form['written'][1] = 'ffffffffffffffff'
        self.mutate(change)
        with self.assertRaisesRegex(ValueError, 'digests differ'):
            analyze.compare(self.root, 'base')

    def test_cross_platform_digests_are_rejected(self):
        self.mutate(lambda d: d['tests']['work']['record'].__setitem__('platform', 'other'))
        with self.assertRaisesRegex(ValueError, 'one platform'):
            analyze.compare(self.root, 'base')

    def test_missing_test_is_rejected(self):
        self.mutate(lambda d: d['tests'].pop('work'))
        with self.assertRaisesRegex(ValueError, 'missing tests'):
            analyze.compare(self.root, 'base')

    def test_form_agreement_is_required(self):
        self.mutate(lambda d: d['tests']['work']['record']['forms']['detached'].__setitem__('differing', 1))
        with self.assertRaisesRegex(ValueError, 'agreement failed'):
            analyze.compare(self.root, 'base')


if __name__ == '__main__':
    unittest.main()
