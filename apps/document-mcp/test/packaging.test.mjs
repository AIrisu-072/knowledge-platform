import test from 'node:test';
import assert from 'node:assert/strict';
import {existsSync} from 'node:fs';
test('build emits Node-runnable adapter and stdio executable',()=>{assert.ok(existsSync(new URL('../dist/main.cjs',import.meta.url)));assert.ok(existsSync(new URL('../dist/adapter.cjs',import.meta.url)));});
