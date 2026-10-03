"""Check the exact approved checksum exceptions; execute only verified Gitleaks/Git."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
import tomllib

SUBJECT = '0802fe6de30c9491c0fe459c57b2c56641f027d9'
BASE = '629822a49657bf447da9061c568dc72558265ce7'
TERMS = 'docs/research/organization-tauri-windows-inventory/runtime-terms-receipt.json'
SHARD = 'docs/research/organization-tauri-windows-inventory/inventory/inactive_or_other_target-007.json'
APPROVED = [f'{SUBJECT}:{TERMS}:generic-api-key:{line}' for line in (10, 21, 32)] + [f'{SUBJECT}:{SHARD}:generic-api-key:1']
BINARY_SHA256 = '88f91962aa2f93ac6ab281d553b9e125f5197bbbce38f9f2437f7299c32e5509'
ENV = {**os.environ, 'GIT_NO_LAZY_FETCH': '1', 'GIT_TERMINAL_PROMPT': '0'}


class ScanUnavailable(ValueError):
    """A scanner/process error, never a successful empty finding result."""


def require(ok, message):
    if not ok:
        raise ValueError(message)


def entries(data):
    return [line for line in data.decode().splitlines() if line and not line.startswith('#')]


def checked_self_test(current, baseline, binary):
    task = tomllib.loads(current)['tasks']['security:secrets:self-test']['run']
    expected = tomllib.loads(baseline)['tasks']['security:secrets:self-test']['run']
    require(isinstance(task, str) and task == expected, 'self-test source drift')
    require(binary.name == 'gitleaks', 'self-test executable name differs from verified input')
    return task


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--gitleaks', type=Path, required=True)
    parser.add_argument('--receipt', type=Path, required=True)
    args = parser.parse_args()
    repo = Path(__file__).resolve().parents[2]
    binary = args.gitleaks.resolve()
    require(hashlib.sha256(binary.read_bytes()).hexdigest() == BINARY_SHA256, 'unverified scanner binary')
    require(subprocess.check_output([str(binary), 'version'], text=True).strip() == '8.30.1', 'scanner version')
    original = subprocess.check_output(['git', 'show', SUBJECT + ':.gitleaksignore'], cwd=repo, env=ENV)
    current = (repo / '.gitleaksignore').read_bytes()
    require(len(entries(original)) == 31 and entries(current) == entries(original) + APPROVED, 'exception scope drift')
    require(current.startswith(original), 'previous31 bytes changed')
    require((repo / '.gitleaks.toml').read_bytes() == subprocess.check_output(['git', 'show', SUBJECT + ':.gitleaks.toml'], cwd=repo, env=ENV), 'rule configuration changed')
    with tempfile.TemporaryDirectory(prefix='tauri-gitleaks-scope-') as temp:
        temp = Path(temp)
        counter = 0

        def scan(path, ignore, options=None):
            nonlocal counter
            counter += 1
            report = temp / f'report-{counter}.json'
            command = [str(binary), 'git', '--redact', '--no-banner', '--no-color', '--timeout', '60', '--exit-code', '1',
                       '--config', str(repo / '.gitleaks.toml'), '--gitleaks-ignore-path', str(ignore),
                       '--report-format', 'json', '--report-path', str(report)]
            if options:
                command.append('--log-opts=' + options)
            command.append(str(path))
            try:
                run = subprocess.run(command, cwd=path, capture_output=True, env=ENV, timeout=75)
            except subprocess.TimeoutExpired as error:
                raise ScanUnavailable('scanner timeout; captured output withheld') from error
            if run.returncode not in (0, 1) or not report.exists() or b' ERR ' in run.stderr:
                raise ScanUnavailable('scanner did not complete; captured output withheld')
            rows = json.loads(report.read_text())
            # Never print Match, Secret, raw detector output or fixture contents.
            return run.returncode, [(r['Fingerprint'], r['RuleID'], r['Commit'], r['File'], r['StartLine']) for r in rows]

        # Gitleaks also reads the source's own ignore file. Isolate both input
        # files; an explicit previous-ignore argument alone is not a reversal.
        subject_view = temp / 'subject-view'
        subprocess.run(['git', 'init', '-q', str(subject_view)], check=True, env=ENV)
        common = subprocess.check_output(['git', 'rev-parse', '--path-format=absolute', '--git-common-dir'],
                                         cwd=repo, text=True, env=ENV).strip()
        (subject_view / '.git/objects/info/alternates').write_text(str(Path(common) / 'objects') + '\n')
        subprocess.run(['git', 'update-ref', 'refs/heads/subject', SUBJECT], cwd=subject_view, check=True, env=ENV)
        subprocess.run(['git', 'symbolic-ref', 'HEAD', 'refs/heads/subject'], cwd=subject_view, check=True, env=ENV)
        subject_ignore = subject_view / '.gitleaksignore'
        subject_ignore.write_bytes(original)
        options = '--full-history --diff-filter=tuxdb ' + BASE + '..' + SUBJECT
        before_exit, before = scan(subject_view, subject_ignore, options)
        require(before_exit == 1 and len(before) == 5 and {r[0] for r in before} == set(APPROVED), 'baseline findings differ')
        subject_ignore.write_bytes(current)
        after_exit, after = scan(subject_view, subject_ignore, options)
        require(after_exit == 0 and not after, 'approved subject still has findings')
        subject_ignore.write_bytes(original)
        restored_exit, restored = scan(subject_view, subject_ignore, options)
        require(restored_exit == 1 and sorted(restored) == sorted(before), 'reversal did not reproduce original findings')
        subject_ignore.write_bytes(current)
        ancestry_count = int(subprocess.check_output(['git', 'rev-list', '--count', SUBJECT],
                                                     cwd=subject_view, text=True, env=ENV))
        try:
            full_exit, full_rows = scan(subject_view, subject_ignore)
        except ScanUnavailable:
            full_history = {'status': 'BLOCKED_NOT_PASS', 'reachable_commits': ancestry_count,
                            'scope': 'complete subject ancestry; network/lazy fetching disabled',
                            'reason': 'local full-history scanner did not complete; captured errors withheld',
                            'required_follow_up': 'fresh hosted full-repository history gate'}
        else:
            require(full_exit == 0 and not full_rows, 'unexpected full-history findings; output withheld')
            full_history = {'status': 'PASS', 'reachable_commits': ancestry_count,
                            'scope': 'complete subject ancestry; no unrelated local unpublished refs',
                            'required_follow_up': 'fresh hosted gate also scans other public refs'}

        fixture = temp / 'fixture'
        fixture.mkdir()
        subprocess.run(['git', 'init', '-q', str(fixture)], check=True, env=ENV)

        def commit(message):
            subprocess.run(['git', 'add', '.'], cwd=fixture, check=True, env=ENV)
            subprocess.run(['git', '-c', 'user.name=Checksum regression', '-c', 'user.email=non-live@example.invalid',
                            'commit', '-qm', message], cwd=fixture, check=True, env=ENV)
            return subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=fixture, text=True, env=ENV).strip()

        probe = hashlib.sha256(b'non-live checksum detector regression').hexdigest()
        pat = ''.join(('ghp_', 'wA9mK2pLxN4vRtQzY6', 'bC8dEfGhJlM0oPq1rS'))
        (fixture / 'new-path.txt').write_text(f'api_key = "{probe}"\ntoken = "{pat}"\n')
        first = commit('non-live new-path detector controls')
        first_exit, first_rows = scan(fixture, repo / '.gitleaksignore')
        require(first_exit == 1 and any(r[1] == 'generic-api-key' for r in first_rows) and
                any(r[1] == 'github-pat' for r in first_rows), 'new path or unrelated PAT rule was suppressed')
        for path, line in ((TERMS, 10), (SHARD, 1)):
            out = fixture / path
            out.parent.mkdir(parents=True, exist_ok=True)
            out.write_text('\n' * (line - 1) + f'api_key = "{probe}"\n')
        second = commit('non-live controls at previously excepted paths and lines')
        second_exit, second_rows = scan(fixture, repo / '.gitleaksignore')
        require(second_exit == 1 and second != SUBJECT, 'new commit was suppressed')
        for path, line in ((TERMS, 10), (SHARD, 1)):
            require(any(r[1:] == ('generic-api-key', second, path, line) for r in second_rows), 'same-location new commit was suppressed')

        baseline_mise = subprocess.check_output(['git', 'show', SUBJECT + ':mise.toml'], cwd=repo, env=ENV).decode()
        task = checked_self_test((repo / 'mise.toml').read_text(), baseline_mise, binary)
        env = {key: value for key, value in ENV.items()
               if key not in ('BASH_ENV', 'ENV') and not key.startswith('BASH_FUNC_')}
        env['PATH'] = str(binary.parent) + os.pathsep + os.environ.get('PATH', '')
        self_test = subprocess.run(['bash', '-c', task], cwd=repo, env=env, capture_output=True, timeout=75)
        require(self_test.returncode == 0, 'unchanged repository scanner self-test failed; output withheld')
        receipt = {'subject_commit': SUBJECT, 'original_entries': 31, 'approved_additions': APPROVED,
                   'source_config_unchanged': True, 'previous_entries_bytes_unchanged': True,
                   'scanner_version': '8.30.1', 'scanner_binary_sha256': BINARY_SHA256,
                   'subject_delta_before': {'exit': before_exit, 'findings': len(before)},
                   'subject_delta_after': {'exit': after_exit, 'findings': len(after)},
                   'subject_delta_reversal': {'exit': restored_exit, 'findings': len(restored)},
                   'synthetic_new_path_and_unrelated_pat_detected': True,
                   'synthetic_same_path_line_new_commit_detected': True,
                   'unchanged_repository_scanner_self_test': 'PASS',
                   'self_test_source_bound_to_subject': True,
                   'self_test_executable_name_bound_to_verified_input': True,
                   'full_history': full_history,
                   'tauri_or_windows_runtime_code_executed': False}
        args.receipt.write_text(json.dumps(receipt, indent=2) + '\n')
        print(json.dumps({'subject_before_after_reversal': [len(before), len(after), len(restored)],
                          'negative_controls': 'PASS', 'unchanged_self_test': 'PASS', 'full_history': full_history}))


if __name__ == '__main__':
    main()
