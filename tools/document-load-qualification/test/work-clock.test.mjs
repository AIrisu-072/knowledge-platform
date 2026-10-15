import test from 'node:test';
import assert from 'node:assert/strict';
import * as budget from '../job-budget.mjs';
test('preparation clock uses elapsed monotonic time so a wall-clock correction cannot refund spent budget',()=>{
 assert.equal(typeof budget.createWorkClock,'function');
 let wall=100000,mono=0;const now=budget.createWorkClock({wallNow:()=>wall,monotonicNow:()=>mono});
 mono=60000;wall=150000;assert.equal(now(),160000);
 mono=65000;wall=150001;assert.equal(now(),165000);
 wall=180000;assert.equal(now(),180000);
});
test('invalid or reversed monotonic clock fails closed',()=>{
 assert.equal(typeof budget.createWorkClock,'function');
 let mono=10;const now=budget.createWorkClock({wallNow:()=>100000,monotonicNow:()=>mono});mono=9;assert.throws(now,/clock/);
 assert.throws(()=>budget.createWorkClock({wallNow:()=>NaN,monotonicNow:()=>0}),/clock/);
});
