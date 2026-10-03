import hashlib
import io
from pathlib import Path
import tarfile
import tempfile
import unittest
import inventory

class ArchiveTests(unittest.TestCase):
    def make_archive(self, entries):
        directory = tempfile.TemporaryDirectory(); self.addCleanup(directory.cleanup)
        path = Path(directory.name) / 'sample.crate'
        with tarfile.open(path, 'w:gz') as tf:
            for name, kind, content in entries:
                info = tarfile.TarInfo(name); info.type = kind
                if kind == tarfile.REGTYPE:
                    info.size = len(content); tf.addfile(info, io.BytesIO(content))
                else:
                    info.linkname = 'sample-1.0.0/lib.rs'; tf.addfile(info)
        return path, hashlib.sha256(path.read_bytes()).hexdigest()

    def test_regular_members_produce_authoritative_hashes(self):
        path, digest = self.make_archive([('sample-1.0.0/lib.rs', tarfile.REGTYPE, b'// source')])
        self.assertEqual(inventory.inspect_archive(path, digest, 'sample-1.0.0'), {'lib.rs': hashlib.sha256(b'// source').hexdigest()})

    def test_safe_directory_members_are_allowed(self):
        path,digest=self.make_archive([('sample-1.0.0/',tarfile.DIRTYPE,b''),('sample-1.0.0/src/',tarfile.DIRTYPE,b''),('sample-1.0.0/src/lib.rs',tarfile.REGTYPE,b'body')])
        try:
            result=inventory.inspect_archive(path,digest,'sample-1.0.0')
        except ValueError:
            self.fail('safe directory archive rejected')
        self.assertEqual(result,{'src/lib.rs':hashlib.sha256(b'body').hexdigest()})

    def test_mismatched_archive_checksum_is_rejected(self):
        path, _ = self.make_archive([])
        with self.assertRaises(ValueError): inventory.inspect_archive(path, '0'*64, 'sample-1.0.0')

    def test_unsafe_paths_and_member_types_are_rejected(self):
        for name, kind in [('../escape', tarfile.REGTYPE),('/absolute',tarfile.REGTYPE),('wrong/lib.rs',tarfile.REGTYPE),('sample-1.0.0/../escape',tarfile.REGTYPE),('sample-1.0.0/a\\b',tarfile.REGTYPE),('sample-1.0.0/link',tarfile.SYMTYPE),('sample-1.0.0/hard',tarfile.LNKTYPE),('sample-1.0.0/fifo',tarfile.FIFOTYPE)]:
            with self.subTest(name=name,kind=kind):
                path,digest=self.make_archive([(name,kind,b'body')])
                with self.assertRaises(ValueError): inventory.inspect_archive(path,digest,'sample-1.0.0')

    def test_duplicate_member_is_rejected(self):
        path,digest=self.make_archive([('sample-1.0.0/lib.rs',tarfile.REGTYPE,b'a'),('sample-1.0.0/lib.rs',tarfile.REGTYPE,b'b')])
        with self.assertRaises(ValueError): inventory.inspect_archive(path,digest,'sample-1.0.0')

    def test_cache_regular_files_equal_archive_and_only_cargo_marker_is_extra(self):
        directory=tempfile.TemporaryDirectory();self.addCleanup(directory.cleanup);root=Path(directory.name)
        root.joinpath('lib.rs').write_bytes(b'body');root.joinpath('.cargo-ok').write_text('{"v":1}')
        self.assertEqual(inventory.verify_cache(root,{'lib.rs':hashlib.sha256(b'body').hexdigest()}),{'.cargo-ok':hashlib.sha256(b'{"v":1}').hexdigest()})

    def test_cache_changed_missing_extra_and_symlink_are_rejected(self):
        for case in ('changed','missing','extra','symlink'):
            with self.subTest(case=case):
                directory=tempfile.TemporaryDirectory();self.addCleanup(directory.cleanup);root=Path(directory.name)
                root.joinpath('lib.rs').write_bytes(b'body')
                expected={'lib.rs':hashlib.sha256(b'body').hexdigest()}
                if case=='changed':root.joinpath('lib.rs').write_bytes(b'other')
                elif case=='missing':root.joinpath('lib.rs').unlink()
                elif case=='extra':root.joinpath('extra.rs').write_text('extra')
                else:root.joinpath('link').symlink_to('lib.rs')
                with self.assertRaises(ValueError):inventory.verify_cache(root,expected)


class RoleTests(unittest.TestCase):
    def test_build_proc_ancestry_and_both_roles(self):
        text = '''root v0.0.0||
|-- shared v1.0.0|MIT|std
|-- macros v1.0.0 (proc-macro)|MIT|
|   `-- helper v1.0.0|MIT|
[build-dependencies]
`-- builder v1.0.0|MIT|
    |-- shared v1.0.0|MIT|alloc
    `-- helper v1.0.0|MIT|
'''
        roles=inventory.classify_tree(text,{('macros','1.0.0')})
        self.assertIn('shared@1.0.0',roles)
        self.assertEqual(roles['shared@1.0.0']['roles'],['host','target'])
        self.assertEqual(roles['shared@1.0.0']['features_by_role'],{'target':['std'],'host':['alloc']})
        self.assertEqual(roles['helper@1.0.0']['roles'],['host'])
        self.assertEqual(roles['builder@1.0.0']['paths']['host'][0],['root@0.0.0','builder@1.0.0'])

    def test_build_section_does_not_leak_to_uncle(self):
        text='''root v0.0.0||
|-- parent v1.0.0|MIT|
|   [build-dependencies]
|   `-- helper v1.0.0|MIT|
`-- target v1.0.0|MIT|
'''
        roles=inventory.classify_tree(text,set())
        self.assertIn('helper@1.0.0',roles)
        self.assertEqual(roles['helper@1.0.0']['roles'],['host'])
        self.assertEqual(roles['target@1.0.0']['roles'],['target'])

    def test_bad_depth_or_duplicate_marker_is_rejected(self):
        for text in ('root v0.0.0||\n        `-- lost v1.0.0|MIT|\n', '[build-dependencies]\nroot v0.0.0||\n', 'root v0.0.0||\n[dev-dependencies]\n'):
            with self.subTest(text=text):
                with self.assertRaises(ValueError):inventory.classify_tree(text,set())

    def test_exact_exception_rejects_wrong_version_target_use_and_unresolved_role(self):
        good={'cssparser@0.37.0':{'roles':['host']},'option-ext@0.2.0':{'roles':['host','target']}}
        self.assertEqual(inventory.check_exception_roles(good,'x86_64-pc-windows-msvc'),[])
        for bad in ({'cssparser@0.37.0':{'roles':['target']}},{'cssparser@0.37.1':{'roles':['host']}},{'option-ext@0.2.0':{'roles':['unknown']}},{'dtoa-short@0.3.5':{'roles':['host','target']}}):
            with self.subTest(bad=bad):self.assertTrue(inventory.check_exception_roles(bad,'x86_64-pc-windows-msvc'))

class TargetScopeTests(unittest.TestCase):
    def test_scoped_exception_is_not_a_linux_target_waiver(self):
        with self.assertRaises(ValueError):
            inventory.check_exception_roles({'option-ext@0.2.0':{'roles':['target']}},'x86_64-unknown-linux-gnu')

    def test_proc_annotation_must_match_metadata(self):
        with self.assertRaises(ValueError):
            inventory.classify_tree('root v0.0.0||\n`-- macros v1.0.0 (proc-macro)|MIT|\n',set())

class CoverageTests(unittest.TestCase):
    def test_lock_only_entries_stay_inactive_and_selected_absence_stops(self):
        packages=[{'name':'active','version':'1.0.0'},{'name':'inactive','version':'1.0.0'}]
        roles={'active@1.0.0':{'roles':['host'],'features_by_role':{},'paths':{},'occurrences':1}}
        result=inventory.merge_roles(packages,roles)
        self.assertIn('inactive@1.0.0',result)
        self.assertEqual(result['inactive@1.0.0']['role_classification'],'inactive_or_other_target')
        self.assertEqual(result['active@1.0.0']['role_classification'],'host_only')
        with self.assertRaises(ValueError):inventory.merge_roles(packages,{'missing@1.0.0':{'roles':['target']}})

    def test_scanner_coverage_cannot_drop_selected_host(self):
        self.assertEqual(inventory.missing_coverage({'host@1','target@1'},{'target@1'}),['host@1'])
        self.assertEqual(inventory.missing_coverage({'host@1'},{'host@1','inactive@1'}),[])

class ObservedTreeTests(unittest.TestCase):
    def test_version_check_name_does_not_parse_branch_as_package(self):
        text='root v0.0.0||\n`-- version_check v0.9.5|MIT/Apache-2.0|\n'
        try:result=inventory.classify_tree(text,set())
        except ValueError:self.fail('real package beginning with v misparsed')
        self.assertIn('version_check@0.9.5',result)

class CaptureTests(unittest.TestCase):
    def test_capture_hash_mismatch_is_rejected(self):
        directory=tempfile.TemporaryDirectory();self.addCleanup(directory.cleanup);root=Path(directory.name)
        root.joinpath('data.json').write_bytes(b'{}')
        with self.assertRaises(ValueError):inventory.check_capture_hashes(root,{'data.json':{'sha256':'0'*64,'bytes':2}})

    def test_capture_hash_and_length_match(self):
        directory=tempfile.TemporaryDirectory();self.addCleanup(directory.cleanup);root=Path(directory.name)
        root.joinpath('data.json').write_bytes(b'{}')
        self.assertEqual(inventory.check_capture_hashes(root,{'data.json':{'sha256':hashlib.sha256(b'{}').hexdigest(),'bytes':2}}),1)

if __name__ == '__main__': unittest.main()
