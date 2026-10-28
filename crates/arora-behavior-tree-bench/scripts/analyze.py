#!/usr/bin/env python3
"""Validate agreement, then compare N builds using a bootstrap over rounds."""
import argparse
import json
import math
from pathlib import Path
import random
import statistics
import sys

RESAMPLES = 2000
SEED = 20261025


def geomean(values):
    return math.exp(statistics.fmean(math.log(x) for x in values))


def load(directory, baseline, chosen=None):
    manifest = json.loads((directory / 'manifest.json').read_text())
    builds, rounds = list(manifest['builds']), manifest['rounds']
    if baseline not in builds or not isinstance(rounds, int) or rounds < 1:
        raise ValueError('invalid baseline or rounds')
    forms = chosen or manifest.get('forms')
    values, digests, platform, layout = {}, {}, None, None
    cells_checked = 0
    for build in builds:
        actual = {p.name for p in (directory / 'runs' / build).glob('*.json')}
        if actual != {f'{r}.json' for r in range(1, rounds + 1)}:
            raise ValueError(f'{build}: missing or extra round files')
        for r in range(1, rounds + 1):
            data = json.loads((directory / 'runs' / build / f'{r}.json').read_text())
            if data.get('schema') != 1 or not data.get('tests'):
                raise ValueError(f'{build}/{r}: invalid round schema or empty tests')
            dimensions = {}
            for name, test in data['tests'].items():
                record = test['record']
                if record.get('schema') != 1:
                    raise ValueError(f'{build}/{r}/{name}: invalid record schema')
                if platform is None:
                    platform = record['platform']
                if record['platform'] != platform:
                    raise ValueError('digests must be compared within one platform')
                available = list(record['forms'])
                if forms is None:
                    forms = available
                if not forms or len(set(forms)) != len(forms) or not set(forms) <= set(available):
                    raise ValueError(f'{build}/{r}/{name}: missing or duplicate forms')
                kind, size = test['kind'], test['size']
                if kind not in ('scale', 'workload') or (kind == 'scale' and (size < 1 or not name.endswith(f'/{size}'))):
                    raise ValueError(f'{name}: invalid kind or size')
                dimensions[name] = (kind, size, record['ticks'], record['warmUp'], record['dt'], record['phases'])
                ticks, warm = record['ticks'], record['warmUp']
                if not 0 <= warm < ticks:
                    raise ValueError(f'{name}: invalid replay dimensions')
                seen = None
                for form in forms:
                    entry = record['forms'][form]
                    written = tuple(entry['written'])
                    if entry['differing'] != 0 or len(written) != ticks:
                        raise ValueError(f'{build}/{r}/{name}/{form}: agreement failed')
                    if any(not isinstance(h, str) or len(h) != 16 or any(c not in '0123456789abcdef' for c in h) for h in written):
                        raise ValueError(f'{name}/{form}: invalid digest')
                    if seen is not None and seen != written:
                        raise ValueError(f'{build}/{r}/{name}: forms disagree')
                    seen = written
                    key = (name, form)
                    if key in digests and digests[key] != written:
                        raise ValueError(f'{build}/{r}/{name}/{form}: digests differ across builds or rounds')
                    digests[key] = written
                    # Agreement is checked before any times are consumed.
                    replays = entry['times']
                    if not replays or len(replays) % 2 != 1 or any(len(x) != ticks for x in replays):
                        raise ValueError(f'{name}/{form}: incomplete timing sample')
                    measured = [x for replay in replays for x in replay[warm:]]
                    if any(not isinstance(x, (float, int)) or not math.isfinite(x) or x < 0 for x in measured) or statistics.fmean(measured) <= 0:
                        raise ValueError(f'{name}/{form}: invalid time')
                    values.setdefault((name, build, form), []).append(statistics.fmean(measured))
                    cells_checked += 1
            if layout is None:
                layout = dimensions
            if layout != dimensions:
                raise ValueError(f'{build}/{r}: missing tests or different replay dimensions')
    return builds, rounds, forms, values, layout, platform, cells_checked


def compare(directory, baseline, forms=None):
    builds, rounds, forms, values, layout, platform, checked = load(directory, baseline, forms)
    groups = {}
    kinds = {}
    for name, (kind, *_rest) in layout.items():
        group = name.rsplit('/', 1)[0] if kind == 'scale' else name
        if group in kinds and kinds[group] != kind:
            raise ValueError(f'test group collision: {group}')
        groups.setdefault(group, []).append(name)
        kinds[group] = kind
    cells = [(b, f) for b in builds for f in forms]

    def ratios(draw):
        means = {key: statistics.fmean(by_round[i] for i in draw[key[1]]) for key, by_round in values.items()}
        return {test: {(b, f): geomean([means[(name, b, f)] / means[(name, baseline, f)] for name in names])
                       for b, f in cells} for test, names in groups.items()}

    aggregates = {'overall': list(groups), 'workloads': [g for g in groups if kinds[g] == 'workload'],
                  'scenarios': [g for g in groups if kinds[g] == 'scale']}
    aggregates = {k: v for k, v in aggregates.items() if v}
    if set(groups) & set(aggregates):
        raise ValueError('test name collides with aggregate name')

    def aggregate(point):
        return {**point, **{name: {cell: geomean([point[t][cell] for t in tests]) for cell in cells}
                           for name, tests in aggregates.items()}}

    point = aggregate(ratios({b: list(range(rounds)) for b in builds}))
    bootstrap = {name: {cell: [] for cell in cells} for name in point}
    rng = random.Random(SEED)
    for _ in range(RESAMPLES):
        # One process supplies all tests in a round: retain that covariance.
        draw = {b: [rng.randrange(rounds) for _ in range(rounds)] for b in builds}
        sample = aggregate(ratios(draw))
        for name in point:
            for cell in cells:
                bootstrap[name][cell].append(sample[name][cell])
    result = {'schema': 1, 'baseline': baseline, 'builds': builds, 'forms': forms, 'rounds': rounds,
              'platform': platform, 'resamples': RESAMPLES, 'seed': SEED, 'agreementCells': checked,
              'tests': {}}
    lines = [f'{rounds} rounds per build; ratios against {baseline}, same form; 95% bootstrap intervals ({RESAMPLES} resamples).',
             f'Agreement: {checked} round × test × form cells agree on {platform}.', '',
             '| test | ' + ' | '.join(f'{b} `{f}`' for b, f in cells) + ' |', '|---|' + '---:|' * len(cells)]
    for name, by_cell in point.items():
        result['tests'][name] = {}
        formatted = []
        for b, f in cells:
            samples = sorted(bootstrap[name][(b, f)])
            interval = [samples[int(0.025 * RESAMPLES)], samples[int(0.975 * RESAMPLES) - 1]]
            ratio = by_cell[(b, f)]
            result['tests'][name][f'{b}/{f}'] = {'ratio': ratio, 'ci': interval}
            formatted.append(f'{ratio:.4f} [{interval[0]:.4f}, {interval[1]:.4f}]')
        lines.append(f'| {name} | ' + ' | '.join(formatted) + ' |')
    return result, '\n'.join(lines) + '\n'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('directory', type=Path)
    parser.add_argument('--baseline', required=True)
    parser.add_argument('--forms', help='comma-separated registered form names')
    args = parser.parse_args()
    try:
        result, table = compare(args.directory, args.baseline, args.forms.split(',') if args.forms else None)
    except (ValueError, KeyError, OSError, TypeError, ZeroDivisionError) as error:
        sys.exit(f'invalid benchmark run: {error}')
    (args.directory / 'compare.json').write_text(json.dumps(result, indent=2) + '\n')
    (args.directory / 'compare.md').write_text(table)
    print(table, end='')


if __name__ == '__main__':
    main()
