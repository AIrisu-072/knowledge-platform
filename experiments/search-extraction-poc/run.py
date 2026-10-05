#!/usr/bin/env python3
"""Verify fixed fixture hashes/oracle, then run one isolated Rust qualifier binary."""
import argparse
import hashlib
import json
import os
import pathlib
import subprocess
import sys

from oracle import known_omissions, inspect

ROOT = pathlib.Path(__file__).resolve().parent


def verify_manifest(rows):
    ids = set()
    formats = set()
    for row in rows:
        key = (row['id'], row['format'])
        if key in ids: raise AssertionError(f'duplicate id/format: {key}')
        ids.add(key); formats.add(row['format'])
        path = ROOT / row['file']
        if path.parent != ROOT / 'fixtures' or not path.is_file(): raise AssertionError(f'missing/unscoped fixture: {key}')
        raw = path.read_bytes()
        if hashlib.sha256(raw).hexdigest() != row['sha256']: raise AssertionError(f'raw hash: {key}')
        if row['origin'] != 'synthetic:generate.py' or row['license'] != 'CC0-1.0': raise AssertionError(f'origin/license: {key}')
        found, coverage, reasons = inspect(row['format'], raw, row['limits'])
        if (found, coverage, reasons) != (row['expected_units'], row['coverage'], row['reasons']):
            raise AssertionError(f'oracle mismatch: {key}: {found, coverage, reasons!r}')
        omissions = known_omissions(row['format'], raw) if coverage in ('Supported', 'Partial') else []
        if omissions != row.get('known_omissions', []):
            raise AssertionError(f'known omission oracle mismatch: {key}: {omissions!r}')
        omitted_locations = {(tuple(item['member_chain']), item['package_path'],
                              tuple(item['physical_child_path'])) for item in omissions}
        if len(omitted_locations) != len(omissions):
            raise AssertionError(f'duplicate formula omission: {key}')
        locators = [json.dumps(x['locator'], ensure_ascii=False, sort_keys=True) for x in found]
        if len(locators) != len(set(locators)): raise AssertionError(f'ambiguous locator: {key}')
        if coverage == 'Supported' and reasons: raise AssertionError(f'Supported reasons: {key}')
        if coverage == 'Supported' and omissions: raise AssertionError(f'Supported formula omissions: {key}')
        if coverage == 'Partial' and (not found or not reasons or not omissions):
            raise AssertionError(f'Partial without located omission: {key}')
        if coverage == 'Unsupported' and (found or len(reasons) != 1): raise AssertionError(f'Unsupported units: {key}')
    required = {'docx', 'xlsx', 'xlsm', 'pptx', 'pdf', 'text', 'csv', 'html', 'zip', 'doc', 'xls', 'ppt'}
    if formats != required: raise AssertionError(f'formats: {formats ^ required}')
    if len(rows) < 20: raise AssertionError('insufficient corpus')
    print(f'manifest/oracle PASS: {len(rows)} raw fixtures', file=sys.stderr)


def qualify(rows, fmt, target):
    if fmt not in {r['format'] for r in rows}: raise ValueError(fmt)
    env = dict(os.environ)
    env.setdefault('CARGO_INCREMENTAL', '0')
    env.setdefault('CARGO_PROFILE_DEV_DEBUG', '0')
    env.setdefault('CARGO_BUILD_JOBS', '2')
    env['CARGO_TARGET_DIR'] = str(target)
    selected = [r for r in rows if r['format'] == fmt]
    request = {'rows': selected, 'root': str(ROOT)}
    p = subprocess.run(
        ['cargo', 'run', '--manifest-path', str(ROOT / 'Cargo.toml'), '--locked', '--bin', 'extraction-qualify'],
        input=json.dumps(request, ensure_ascii=False).encode(), stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=env)
    if p.stderr: sys.stderr.buffer.write(p.stderr)
    parsed = [json.loads(line) for line in p.stdout.splitlines()]
    if len(parsed) != len(selected): raise AssertionError('qualifier did not emit one result per fixture')
    if p.returncode not in (0, 1): raise RuntimeError(f'qualifier crashed: {p.returncode}')
    for row, actual in zip(selected, parsed):
        if row['id'] != actual['fixture_id'] or actual['format'] != fmt: raise AssertionError('qualifier row identity')
        print(json.dumps(actual, ensure_ascii=False, sort_keys=True))
    if not all(result['qualified'] for result in parsed):
        raise SystemExit(f'{fmt} qualification failed; inspect the emitted JSONL')


if __name__ == '__main__':
    a = argparse.ArgumentParser()
    a.add_argument('--verify-manifest', action='store_true')
    a.add_argument('--qualify', action='store_true')
    a.add_argument('--format', choices=['docx', 'xlsx', 'xlsm', 'pptx', 'pdf', 'text', 'csv', 'html', 'zip', 'doc', 'xls', 'ppt'])
    a.add_argument('--target', type=pathlib.Path, default=ROOT / 'target')
    args = a.parse_args()
    rows = json.loads((ROOT / 'manifest.json').read_text())
    verify_manifest(rows)
    if args.qualify:
        if not args.format: a.error('--format required with --qualify')
        qualify(rows, args.format, args.target)
