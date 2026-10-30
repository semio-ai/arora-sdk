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
        self.manifest = {'schema': 1, 'rounds': 4, 'builds': {'base': 'a', 'second': 'b', 'third': 'c'}}
        (self.root / 'manifest.json').write_text(json.dumps(self.manifest))
        for build, ratio in [('base', 1), ('second', 2), ('third', 0.5)]:
            folder = self.root / 'runs' / build
            folder.mkdir(parents=True)
            for round_ in range(1, 5):
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
        self.assertEqual(result['agreementCells'], 72)
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
        self.assertEqual(result['agreementCells'], 36)

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

    def all_records(self, change):
        for path in (self.root / 'runs').glob('*/*.json'):
            data = json.loads(path.read_text())
            change(data)
            path.write_text(json.dumps(data))

    def test_too_few_and_unbalanced_rounds(self):
        for rounds in [1, 2, 3, 5]:
            manifest = {**self.manifest, 'rounds': rounds}
            (self.root / 'manifest.json').write_text(json.dumps(manifest))
            with self.assertRaisesRegex(ValueError, 'rounds|odd round'):
                analyze.load(self.root, 'base')
        for build in self.manifest['builds']:
            self.put(build, 5, json.loads(self.path(build, 4).read_text()))
        self.assertEqual(analyze.load(self.root, 'base', allow_odd_rounds=True)[1], 5)

    def test_extra_round_is_rejected(self):
        self.put('second', 5, json.loads(self.path().read_text()))
        with self.assertRaisesRegex(ValueError, 'extra round files'):
            analyze.load(self.root, 'base')

    def test_disagreement_within_first_record_is_rejected(self):
        data = json.loads(self.path('base').read_text())
        data['tests']['work']['record']['forms']['attached-naive']['written'][0] = 'ffffffffffffffff'
        self.put('base', 1, data)
        with self.assertRaisesRegex(ValueError, 'forms disagree'):
            analyze.load(self.root, 'base')

    def test_nonfinite_and_negative_times_are_rejected(self):
        for value in [float('nan'), float('inf'), -1]:
            self.mutate(lambda d: d['tests']['work']['record']['forms']['detached']['times'][0].__setitem__(1, value))
            with self.assertRaisesRegex(ValueError, 'invalid time'):
                analyze.load(self.root, 'base')

    def test_invalid_kind_and_size_are_rejected(self):
        for field, value in [('kind', 'unknown'), ('size', 0), ('size', 11)]:
            original = json.loads(self.path().read_text())
            self.mutate(lambda d: d['tests']['chain/10'].__setitem__(field, value))
            with self.assertRaisesRegex(ValueError, 'invalid kind or size'):
                analyze.load(self.root, 'base')
            self.put('second', 1, original)

    def test_digest_length_must_equal_ticks(self):
        self.all_records(lambda d: [test['record']['forms'][form]['written'].pop()
            for test in d['tests'].values() for form in test['record']['forms']])
        with self.assertRaisesRegex(ValueError, 'agreement failed'):
            analyze.load(self.root, 'base')

    def test_incomplete_replay_count_or_shape_is_rejected(self):
        for times in [[], [[1, 2, 3]] * 2, [[1, 2]]]:
            self.mutate(lambda d: d['tests']['work']['record']['forms']['detached'].__setitem__('times', times))
            with self.assertRaisesRegex(ValueError, 'incomplete timing sample'):
                analyze.load(self.root, 'base')

    def test_digest_format_is_checked(self):
        self.all_records(lambda d: [test['record']['forms'][form].__setitem__('written', ['not-a-digest'] * 3)
            for test in d['tests'].values() for form in test['record']['forms']])
        with self.assertRaisesRegex(ValueError, 'invalid digest'):
            analyze.load(self.root, 'base')

    def test_group_collision_is_rejected(self):
        self.all_records(lambda d: d['tests'].__setitem__('chain', d['tests']['work']))
        with self.assertRaisesRegex(ValueError, 'group collision'):
            analyze.compare(self.root, 'base')

    def test_replay_intervals_supply_round_means(self):
        def change(d):
            for test in d['tests'].values():
                for entry in test['record']['forms'].values():
                    entry['sampleMeans'] = [sum(entry['times'][0][1:]) / 2]
                    entry['times'] = [[]]
                    entry['timingMode'] = ['replay']
        self.all_records(change)
        result, _ = analyze.compare(self.root, 'base')
        self.assertEqual(result['tests']['work']['second/detached']['ratio'], 2)


if __name__ == '__main__':
    unittest.main()
