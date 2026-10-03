"""Lossless, bounded inventory storage. Uses only Python's standard library."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import sys

SCHEMA = 'organization-tauri-inventory-shards-v1'
LIMIT = 40 * 1024  # Every stored file must be strictly smaller.
GROUPS = {'fixture', 'host_only', 'host_and_target', 'normal_target_only',
          'inactive_or_other_target'}


def require(condition, message):
    if not condition:
        raise ValueError(message)


def encoded(value):
    return (json.dumps(value, sort_keys=True, separators=(',', ':'),
                       allow_nan=False) + '\n').encode()


def digest(data):
    return hashlib.sha256(data).hexdigest()


def git_blob(data):
    return hashlib.sha1(b'blob ' + str(len(data)).encode() + b'\0' + data).hexdigest()


def unique_pairs(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result, 'duplicate JSON key')
        result[key] = value
    return result


def reject_constant(value):
    raise ValueError('non-finite JSON number: ' + value)


def parse(data):
    return json.loads(data, object_pairs_hook=unique_pairs, parse_constant=reject_constant)


def role_group(row):
    require(isinstance(row, dict), 'invalid package record')
    group = row.get('role_classification')
    require(isinstance(group, str) and group in GROUPS - {'fixture'}, 'unknown package role')
    return group if row.get('source') else 'fixture'


def render_inventory_shards(data, max_file_bytes=32768):
    require(type(max_file_bytes) is int and 0 < max_file_bytes < LIMIT, 'invalid file bound')
    require(isinstance(data, dict) and set(data) == {'packages', 'summary'}, 'invalid inventory')
    require(isinstance(data['packages'], dict) and isinstance(data['summary'], dict), 'invalid inventory maps')
    original = encoded(data)
    grouped = {}
    for key, row in sorted(data['packages'].items()):
        require(isinstance(key, str) and key, 'invalid package ID')
        grouped.setdefault(role_group(row), {})[key] = row
    files, shards = {}, []
    for group, packages in sorted(grouped.items()):
        chunks, chunk = [], {}
        for key, row in packages.items():
            candidate = {**chunk, key: row}
            if len(encoded({'role_group': group, 'packages': candidate})) > max_file_bytes:
                require(chunk, 'single package exceeds shard bound')
                chunks.append(chunk)
                candidate = {key: row}
                require(len(encoded({'role_group': group, 'packages': candidate})) <= max_file_bytes,
                        'single package exceeds shard bound')
            chunk = candidate
        if chunk:
            chunks.append(chunk)
        for number, chunk in enumerate(chunks, 1):
            path = f'{group}-{number:03d}.json'
            content = encoded({'role_group': group, 'packages': chunk})
            files[path] = content
            shards.append({'path': path, 'role_group': group, 'bytes': len(content),
                           'sha256': digest(content), 'package_count': len(chunk)})
    index = {'schema': SCHEMA, 'summary': data['summary'], 'package_count': len(data['packages']),
             'original_bytes': len(original), 'original_sha256': digest(original),
             'original_git_blob_sha1': git_blob(original), 'shards': shards}
    files['index.json'] = encoded(index)
    require(all(len(content) <= max_file_bytes for content in files.values()), 'index exceeds bound')
    return files


def bounded_read(path):
    require(not path.is_symlink() and path.is_file(), 'missing, symlink or non-file input')
    require(path.stat().st_size < LIMIT, 'stored file exceeds bound')
    with path.open('rb') as stream:
        data = stream.read(LIMIT)
    require(len(data) < LIMIT, 'stored file exceeds bound')
    return data


def load_inventory_shards(index_path):
    path = Path(index_path)
    require(path.name == 'index.json' and not any(parent.is_symlink() for parent in path.parents),
            'invalid or symlinked index path')
    index = parse(bounded_read(path))
    require(isinstance(index, dict) and set(index) == {'schema', 'summary', 'package_count',
            'original_bytes', 'original_sha256', 'original_git_blob_sha1', 'shards'}, 'invalid index fields')
    require(index['schema'] == SCHEMA and isinstance(index['summary'], dict), 'invalid index schema')
    require(isinstance(index['shards'], list), 'invalid shard list')
    for field in ('package_count', 'original_bytes'):
        require(type(index[field]) is int and index[field] >= 0, 'invalid index count')
    packages, paths = {}, set()
    for shard in index['shards']:
        require(isinstance(shard, dict) and set(shard) == {'path', 'role_group', 'bytes',
                'sha256', 'package_count'}, 'invalid shard fields')
        name, group = shard['path'], shard['role_group']
        require(isinstance(name, str) and isinstance(group, str) and group in GROUPS,
                'invalid shard name or group')
        require(re.fullmatch(re.escape(group) + r'-[0-9]{3,}\.json', name) is not None,
                'unsafe or mismatched shard path')
        require(name not in paths, 'duplicate shard path')
        paths.add(name)
        require(type(shard['bytes']) is int and 0 < shard['bytes'] < LIMIT, 'invalid shard size')
        require(type(shard['package_count']) is int and shard['package_count'] > 0, 'invalid shard count')
        content = bounded_read(path.parent / name)
        require(len(content) == shard['bytes'] and digest(content) == shard['sha256'], 'shard checksum mismatch')
        value = parse(content)
        require(isinstance(value, dict) and set(value) == {'role_group', 'packages'} and
                value['role_group'] == group and isinstance(value['packages'], dict), 'invalid shard object')
        require(len(value['packages']) == shard['package_count'], 'shard count mismatch')
        for key, row in value['packages'].items():
            require(isinstance(key, str) and key and key not in packages, 'duplicate or invalid package ID')
            require(role_group(row) == group, 'package role mismatch')
            packages[key] = row
    require({item.name for item in path.parent.iterdir()} == paths | {'index.json'}, 'missing or unindexed file')
    require(len(packages) == index['package_count'], 'inventory count mismatch')
    result = {'packages': packages, 'summary': index['summary']}
    original = encoded(result)
    require(len(original) == index['original_bytes'] and digest(original) == index['original_sha256'] and
            git_blob(original) == index['original_git_blob_sha1'], 'original identity mismatch')
    return result


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('index', type=Path, help='committed inventory/index.json')
    args = parser.parse_args()
    sys.stdout.buffer.write(encoded(load_inventory_shards(args.index)))
