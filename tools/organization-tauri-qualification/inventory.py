"""Data-only checks for the isolated Tauri dependency inventory."""
import hashlib
from pathlib import PurePosixPath
import tarfile


def inspect_archive(path, expected_sha256, prefix):
    if hashlib.sha256(path.read_bytes()).hexdigest() != expected_sha256:
        raise ValueError('archive checksum mismatch')
    files, seen = {}, set()
    with tarfile.open(path) as archive:
        for member in archive:
            parts = member.name.split('/')
            if (member.name.startswith('/') or '\\' in member.name
                    or any(p in ('', '.', '..') for p in parts)
                    or parts[0] != prefix or member.name in seen
                    or not (member.isfile() or member.isdir())):
                raise ValueError('unsafe or duplicate archive member')
            seen.add(member.name)
            if member.isfile():
                if len(parts) < 2:
                    raise ValueError('file replaces archive root')
                relative = str(PurePosixPath(*parts[1:]))
                stream = archive.extractfile(member)
                digest = hashlib.file_digest(stream, 'sha256').hexdigest()
                files[relative] = digest
    return files


def verify_cache(root, expected):
    actual, bookkeeping = {}, {}
    for path in root.rglob('*'):
        if path.is_symlink() or not (path.is_dir() or path.is_file()):
            raise ValueError('unsafe cache member')
        if path.is_file():
            relative = path.relative_to(root).as_posix()
            digest = hashlib.sha256(path.read_bytes()).hexdigest()
            if relative == '.cargo-ok' and relative not in expected:
                bookkeeping[relative] = digest
            else:
                actual[relative] = digest
    if actual != expected:
        raise ValueError('cache source does not equal verified archive')
    return bookkeeping



def classify_tree(text, proc_macros):
    import re
    stack, result = [], {}
    for line in text.splitlines():
        if not line:
            continue
        if '[build-dependencies]' in line:
            prefix=line.removesuffix('[build-dependencies]')
            if len(prefix)%4 or any(c not in ' |`\\+-' for c in prefix):
                raise ValueError('invalid build marker prefix')
            depth=len(prefix)//4
            if len(stack)<=depth:
                raise ValueError('build marker lacks parent')
            stack=stack[:depth+1]
            if stack[-1]['build']:
                raise ValueError('duplicate build marker')
            stack[-1]['build']=True
            continue
        match=re.search(r'([A-Za-z0-9][A-Za-z0-9_-]*) v([0-9][^ |]*)',line)
        if not match:
            raise ValueError('unknown tree line')
        prefix=line[:match.start()]
        if len(prefix)%4 or any(c not in ' |`\\+-' for c in prefix):
            raise ValueError('invalid package prefix')
        depth=len(prefix)//4
        if depth>len(stack) or (depth==0 and stack):
            raise ValueError('invalid tree depth')
        stack=stack[:depth]
        fields=line[match.start():].split('|')
        if len(fields)!=3 or '(*)' in line:
            raise ValueError('ambiguous or deduplicated tree line')
        name,version=match.groups();key=f'{name}@{version}'
        if ('(proc-macro)' in fields[0]) != ((name,version) in proc_macros):
            raise ValueError('proc macro annotation and metadata disagree')
        role='host' if ((name,version) in proc_macros or (stack and (stack[-1]['role']=='host' or stack[-1]['build']))) else 'target'
        path=[node['key'] for node in stack]+[key]
        row=result.setdefault(key,{'roles':set(),'features_by_role':{},'paths':{},'occurrences':0})
        row['roles'].add(role)
        row['features_by_role'].setdefault(role,set()).update(f for f in fields[2].split(',') if f)
        paths=row['paths'].setdefault(role,[])
        if path not in paths and len(paths)<3:
            paths.append(path)
        row['occurrences']+=1
        stack.append({'key':key,'role':role,'build':False})
    for row in result.values():
        row['roles']=sorted(row['roles'])
        row['features_by_role']={role:sorted(features) for role,features in row['features_by_role'].items()}
    return result


EXCEPTIONS={
    'cssparser':('0.37.0',{'host'}),
    'selectors':('0.38.0',{'host'}),
    'cssparser-macros':('0.7.0',{'host'}),
    'dtoa-short':('0.3.5',{'host'}),
    'option-ext':('0.2.0',{'host','target'}),
}


def check_exception_roles(packages, target):
    if target != 'x86_64-pc-windows-msvc':
        raise ValueError('scoped inventory is Windows x64 projection only')
    errors=[]
    for key,row in packages.items():
        name,version=key.rsplit('@',1)
        if name in EXCEPTIONS:
            approved,roles=EXCEPTIONS[name]
            actual=set(row['roles'])
            if version!=approved or not actual or not actual<=roles:
                errors.append(key)
    return errors


def merge_roles(packages,roles):
    result={f"{p['name']}@{p['version']}":dict(p) for p in packages}
    if len(result)!=len(packages) or set(roles)-set(result):
        raise ValueError('duplicate lock identity or selected identity missing from lock')
    classes={frozenset({'host'}):'host_only',frozenset({'target'}):'normal_target_only',frozenset({'host','target'}):'host_and_target',frozenset():'inactive_or_other_target'}
    for key,row in result.items():
        role=roles.get(key,{'roles':[],'features_by_role':{},'paths':{},'occurrences':0})
        if frozenset(role['roles']) not in classes:
            raise ValueError('unresolved selected role')
        row.update(role)
        row['role_classification']=classes[frozenset(role['roles'])]
    return result


def missing_coverage(selected,covered):
    return sorted(set(selected)-set(covered))


def check_capture_hashes(root,entries):
    for name,row in entries.items():
        if '/' in name or '\\' in name or name in ('.','..'):
            raise ValueError('capture input name is not a filename')
        path=root/name
        if path.is_symlink() or not path.is_file():
            raise ValueError('capture input is not a regular file')
        data=path.read_bytes()
        if len(data)!=row['bytes'] or hashlib.sha256(data).hexdigest()!=row['sha256']:
            raise ValueError('capture hash or size mismatch')
    return len(entries)
