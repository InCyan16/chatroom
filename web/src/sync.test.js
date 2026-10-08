import assert from "node:assert/strict";
import test from "node:test";
import { snapshotSync } from "./sync.js";

function deferred() {
  let resolve, reject;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  return {promise, resolve, reject};
}
test("resuming replaces stale conversations with every joined room and private peer", async () => {
  let state = {rooms:[{name:"one"}], private_peers:[]};
  const fresh = {rooms:[{name:"one"},{name:"two",messages:[]}], private_peers:["bob","eve"]};
  let baseline;
  let calls = 0;
  const response = deferred();
  const sync = snapshotSync(() => { calls++; return response.promise; }, (snapshot, quiet) => {state = snapshot; baseline = quiet;}, assert.fail);
  const refresh = sync.refresh();
  assert.equal(sync.refresh(), refresh, "focus and visibility events share one request");
  response.resolve(fresh);
  await refresh;
  assert.equal(calls, 1);
  assert.equal(state, fresh);
  assert.equal(baseline, true, "history refresh must not replay mention notifications");
});
test("a delayed HTTP response cannot hide rooms delivered by WebSocket", async () => {
  const response = deferred();
  const states = [];
  const sync = snapshotSync(() => response.promise, data => states.push(data), assert.fail);
  const refresh = sync.refresh();
  const newer = {rooms:[{name:"one"},{name:"two"}],private_peers:["bob"]};
  sync.receive(newer);
  response.resolve({rooms:[{name:"one"}],private_peers:[]});
  await refresh;
  assert.deepEqual(states, [newer]);
});
test("signing out ignores pending refreshes, and errors allow a later retry", async () => {
  const response = deferred();
  const states = [], errors = [];
  const sync = snapshotSync(() => response.promise, data => states.push(data), error => errors.push(error));
  const refresh = sync.refresh();
  sync.stop();
  response.resolve({rooms:[{name:"private"}]});
  await refresh;
  sync.receive({rooms:[]});
  await sync.refresh();
  assert.deepEqual(states, []);
  assert.deepEqual(errors, []);
  let fail = true;
  const retry = snapshotSync(async () => { if (fail) throw new Error("Please log in."); return {rooms:[]}; }, data => states.push(data), error => errors.push(error.message));
  await retry.refresh();
  assert.deepEqual(errors, ["Please log in."]);
  fail = false;
  await retry.refresh();
  assert.deepEqual(states, [{rooms:[]}]);
});
