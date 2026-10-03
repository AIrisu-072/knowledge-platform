import hashlib
import json
from pathlib import Path
import tempfile
import unittest
from inventory_shards import render_inventory_shards, load_inventory_shards


def encoded(value):
    return (json.dumps(value, sort_keys=True, separators=(',', ':'), allow_nan=False)+'\n').encode()


def sample():
    return {'summary': {'all': 3, 'note': 'all original values stay'}, 'packages': {
        'local@0': {'name': 'local', 'version': '0', 'role_classification': 'normal_target_only', 'roles': ['target']},
        'alpha@1': {'name': 'alpha', 'version': '1', 'source': 'registry', 'role_classification': 'host_only', 'roles': ['host'], 'features_by_role': {'host': ['one','two']}},
        'beta@2': {'name': 'beta', 'version': '2', 'source': 'registry', 'role_classification': 'inactive_or_other_target', 'roles': []},
    }}


class ShardTests(unittest.TestCase):
    def setUp(self):
        self.directory=tempfile.TemporaryDirectory();self.addCleanup(self.directory.cleanup)
        self.root=Path(self.directory.name)

    def write(self, files):
        for name,data in files.items():self.root.joinpath(name).write_bytes(data)

    def fixture(self):
        data=sample();packages=data['packages'];shards=[]
        for group,key in [('fixture','local@0'),('host_only','alpha@1'),('inactive_or_other_target','beta@2')]:
            path=group+'-001.json';content=encoded({'role_group':group,'packages':{key:packages[key]}})
            self.root.joinpath(path).write_bytes(content)
            shards.append({'path':path,'role_group':group,'bytes':len(content),'sha256':hashlib.sha256(content).hexdigest(),'package_count':1})
        original=encoded(data)
        index={'schema':'organization-tauri-inventory-shards-v1','summary':data['summary'],'package_count':3,'original_bytes':len(original),'original_sha256':hashlib.sha256(original).hexdigest(),'original_git_blob_sha1':hashlib.sha1(b'blob '+str(len(original)).encode()+b'\0'+original).hexdigest(),'shards':shards}
        self.root.joinpath('index.json').write_bytes(encoded(index));return data,index

    def test_deterministic_role_grouped_lossless_round_trip(self):
        data=sample();files=render_inventory_shards(data)
        self.assertIn('index.json',files)
        self.assertTrue(all(len(value)<40*1024 for value in files.values()))
        reversed_data={'packages':dict(reversed(list(data['packages'].items()))),'summary':data['summary']}
        self.assertEqual(files,render_inventory_shards(reversed_data))
        self.write(files)
        self.assertEqual(encoded(load_inventory_shards(self.root/'index.json')),encoded(data))

    def test_independent_fixture_reconstructs_all_values(self):
        data,_=self.fixture()
        self.assertEqual(load_inventory_shards(self.root/'index.json'),data)

    def test_multiple_chunks_preserve_every_record_and_are_replayable(self):
        data=sample()
        for number in range(40):
            data['packages'][f'package-{number}@1']={**data['packages']['alpha@1'],'detail':'x'*160}
        files=render_inventory_shards(data,max_file_bytes=2400)
        self.assertGreater(len(files),4)
        self.assertTrue(all(len(value)<=2400 for value in files.values()))
        self.write(files)
        restored=load_inventory_shards(self.root/'index.json')
        self.assertEqual(restored,data)
        self.assertEqual(render_inventory_shards(restored,max_file_bytes=2400),files)

    def test_invalid_schema_counts_blob_hash_and_bound_are_rejected(self):
        for key,value in [('schema','unknown'),('package_count',True),('original_bytes',0),('original_git_blob_sha1','0'*40)]:
            with self.subTest(key=key):
                _,index=self.fixture();index[key]=value;self.root.joinpath('index.json').write_bytes(encoded(index))
                with self.assertRaises(ValueError):load_inventory_shards(self.root/'index.json')
        for limit in (0,-1,40*1024,True):
            with self.subTest(limit=limit):
                with self.assertRaises(ValueError):render_inventory_shards(sample(),limit)

    def test_checksum_mismatch_is_rejected(self):
        self.fixture();self.root.joinpath('host_only-001.json').write_bytes(b'{}')
        with self.assertRaises(ValueError):load_inventory_shards(self.root/'index.json')

    def test_missing_and_unindexed_shards_are_rejected(self):
        for case in ('missing','extra'):
            with self.subTest(case=case):
                self.fixture()
                if case=='missing':self.root.joinpath('host_only-001.json').unlink()
                else:self.root.joinpath('unindexed.json').write_text('{}')
                with self.assertRaises(ValueError):load_inventory_shards(self.root/'index.json')
                if case=='extra':self.root.joinpath('unindexed.json').unlink()

    def test_path_escape_and_symlink_are_rejected(self):
        _,index=self.fixture();index['shards'][0]['path']='../escape.json';self.root.joinpath('index.json').write_bytes(encoded(index))
        with self.assertRaises(ValueError):load_inventory_shards(self.root/'index.json')
        self.fixture();self.root.joinpath('host_only-001.json').unlink();self.root.joinpath('host_only-001.json').symlink_to('fixture-001.json')
        with self.assertRaises(ValueError):load_inventory_shards(self.root/'index.json')

    def test_duplicate_keys_and_duplicate_packages_are_rejected(self):
        self.fixture();self.root.joinpath('index.json').write_bytes(b'{"schema":"a","schema":"b"}')
        with self.assertRaises(ValueError):load_inventory_shards(self.root/'index.json')
        _,index=self.fixture();row=index['shards'][1];row['path']='host_only-002.json';self.root.joinpath(row['path']).write_bytes(self.root.joinpath('host_only-001.json').read_bytes());index['shards'].append(dict(row));index['shards'][1]['path']='host_only-001.json';self.root.joinpath('index.json').write_bytes(encoded(index))
        with self.assertRaises(ValueError):load_inventory_shards(self.root/'index.json')

    def test_oversize_and_wrong_role_or_original_hash_are_rejected(self):
        data=sample();data['packages']['alpha@1']['oversize']='x'*(40*1024)
        with self.assertRaises(ValueError):render_inventory_shards(data)
        _,index=self.fixture();index['original_sha256']='0'*64;self.root.joinpath('index.json').write_bytes(encoded(index))
        with self.assertRaises(ValueError):load_inventory_shards(self.root/'index.json')
        _,index=self.fixture();path=self.root/'host_only-001.json';bad=json.loads(path.read_text());bad['packages']['alpha@1']['role_classification']='normal_target_only';content=encoded(bad);path.write_bytes(content);index['shards'][1]['sha256']=hashlib.sha256(content).hexdigest();index['shards'][1]['bytes']=len(content);self.root.joinpath('index.json').write_bytes(encoded(index))
        with self.assertRaises(ValueError):load_inventory_shards(self.root/'index.json')


if __name__=='__main__':unittest.main()
