"""Verify bounded receipts against original local raw data; never run package code."""
import argparse
import hashlib
import json
from pathlib import Path
import tomllib

from inventory import (check_capture_hashes, check_exception_roles, classify_tree,
                       inspect_archive, merge_roles, missing_coverage, verify_cache)
from inventory_shards import load_inventory_shards


def require(condition, message):
    if not condition:
        raise ValueError(message)


def canonical_digest(value):
    data = json.dumps(value, sort_keys=True, separators=(',', ':')).encode()
    return hashlib.sha256(data).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--raw', type=Path, required=True)
    parser.add_argument('--cargo-home', type=Path, required=True)
    args = parser.parse_args()
    fixture = Path(__file__).resolve().parent
    repo = fixture.parents[1]
    evidence = repo / 'docs/research/organization-tauri-windows-inventory'
    receipt = json.loads((evidence / 'verification-receipt.json').read_text())
    require(check_capture_hashes(args.raw, receipt['raw_inputs']) == len(receipt['raw_inputs']), 'capture coverage')
    lock_bytes = (fixture / 'Cargo.lock').read_bytes()
    require(hashlib.sha256(lock_bytes).hexdigest() == receipt['final_lock_sha256'], 'lock drift')
    inventory = load_inventory_shards(evidence / 'inventory/index.json')
    metadata = json.loads((args.raw / 'metadata-all.json').read_text())
    by_id = {p['name'] + '@' + p['version']: p for p in metadata['packages']}
    macros = {(p['name'], p['version']) for p in metadata['packages'] if any('proc-macro' in t['kind'] for t in p['targets'])}
    roles = classify_tree((args.raw / 'tree-windows.txt').read_text(), macros)
    normal = classify_tree((args.raw / 'tree-normal-no-proc.txt').read_text(), set())
    require({k for k, v in roles.items() if 'target' in v['roles']} == set(normal), 'normal projection disagreement')
    require(not missing_coverage(roles, by_id), 'selected metadata gap')
    require(not check_exception_roles(roles, 'x86_64-pc-windows-msvc'), 'exception scope mismatch')
    cache_root = args.cargo_home.resolve() / 'registry'
    verified_files = selected_files = 0
    packages = tomllib.loads(lock_bytes.decode())['package']
    classified = merge_roles(packages, roles)
    for package in packages:
        key = package['name'] + '@' + package['version']
        row = inventory['packages'][key]
        require(row['roles'] == roles.get(key, {'roles': []})['roles'], 'role receipt drift')
        require(row['role_classification'] == classified[key]['role_classification'], 'role classification drift')
        require(row['paths'] == {r: paths[:1] for r, paths in roles.get(key, {'paths': {}})['paths'].items()}, 'representative path drift')
        require(row['features_by_role'] == roles.get(key, {'features_by_role': {}})['features_by_role'], 'role feature drift')
        if 'source' not in package:
            continue
        require(package['source'] == 'registry+https://github.com/rust-lang/crates.io-index', 'unapproved source')
        require(row['checksum'] == package['checksum'], 'archive receipt drift')
        candidates = list((cache_root / 'cache').glob('*/' + package['name'] + '-' + package['version'] + '.crate'))
        require(len(candidates) == 1, 'ambiguous or absent archive')
        hashes = inspect_archive(candidates[0], package['checksum'], package['name'] + '-' + package['version'])
        require(canonical_digest(hashes) == row['source_member_hash_manifest_sha256'], 'member digest drift')
        require(len(hashes) == row['archive_regular_files'], 'package file count drift')
        for name, digest in row['notice_hashes'].items():
            require(hashes.get(name) == digest, 'notice hash drift')
        for name, digest in row.get('selected_package_embedded_binary_hashes', {}).items():
            require(hashes.get(name) == digest, 'native payload hash drift')
        source = Path(by_id[key]['manifest_path']).resolve().parent
        require(source.is_relative_to(cache_root / 'src'), 'source outside chosen cache')
        require(verify_cache(source, hashes) == row['cargo_bookkeeping_hashes'], 'bookkeeping drift')
        require(row['declared_spdx'] == by_id[key]['license'], 'license declaration drift')
        verified_files += len(hashes)
        if key in roles:
            selected_files += len(hashes)
    selected = {key for key in roles if inventory['packages'][key].get('source')}
    scan_receipt = json.loads((evidence / 'scan-receipt.json').read_text())
    require(hashlib.sha256((repo / 'deny.toml').read_bytes()).hexdigest() == scan_receipt['raw_root_policy_sha256'], 'root policy drift')
    require(hashlib.sha256((fixture / 'deny.windows-qualification.toml').read_bytes()).hexdigest() == scan_receipt['scoped_policy_sha256'], 'scoped policy drift')
    for scope in ('windows', 'all'):
        listing = json.loads((args.raw / f'scanner-{scope}-packages.json').read_text())
        covered = {key.split()[0] + '@' + key.split()[1] for key in listing if ' registry+' in key}
        require(not missing_coverage(selected, covered), 'scanner coverage gap')
    for scope in ('windows-raw', 'all-raw', 'windows-scoped'):
        records = [json.loads(line) for line in (args.raw / f'scan-{scope}.jsonl').read_text().splitlines()]
        summary = next(row['fields'] for row in records if row.get('type') == 'summary')
        require(summary == scan_receipt['results'][scope]['summary'], 'scan summary drift')
        diagnostics = []
        for record in records:
            field = record.get('fields', {})
            if record.get('type') == 'diagnostic':
                diagnostics.append({'severity': field.get('severity'), 'code': field.get('code'),
                                    'message': field.get('message'),
                                    'packages': [x['Krate']['name'] + '@' + x['Krate']['version'] for x in field.get('graphs', []) if 'Krate' in x],
                                    'advisory': field.get('advisory', {}).get('id')})
        require(diagnostics == scan_receipt['results'][scope]['diagnostics'], 'scan diagnostic drift')
        require(int((args.raw / f'scan-{scope}.exit').read_text()) == scan_receipt['results'][scope]['exit_code'], 'scan exit drift')
    require(verified_files == inventory['summary']['all_archive_regular_files_verified'], 'total file count drift')
    require(selected_files == inventory['summary']['selected_archive_regular_files_verified'], 'selected file count drift')
    print(json.dumps({'verified_registry_archives': len(packages)-1, 'verified_regular_files': verified_files,
                      'selected_registry_packages': len(selected), 'selected_regular_files': selected_files,
                      'metadata_source_role_scanner_capture_integrity': 'PASS', 'package_code_executed': False}, indent=2))


if __name__ == '__main__':
    main()
