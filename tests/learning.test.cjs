// Learning tests cover atomic evidence publication, idempotent retry and cross-scope isolation.
const test = require('node:test');
const assert = require('node:assert/strict');
const { fixture, report, seed, counts } = require('./helpers.cjs');
const { learnResearch } = require('../dist/learning');
const { KrakenStore } = require('../dist/store');
const { readResearch } = require('../dist/research-store');

test('three-source agreement remains unverified and retries reuse the same evidence', async (t) => {
  const { store } = fixture(t);
  seed(store);
  const result = await learnResearch(store, 'fixture-request');
  assert.equal(result.status, 'learned');
  assert.equal(result.verified, false);
  assert.equal(result.sources.length, 3);
  assert.equal(result.insights.length, 1);
  assert.equal(store.memory.reasoning.latestConclusion(result.insights[0].id).isVerified, false);
  assert.equal(store.memory.getInsight(result.insights[0].id).status, 'active');
  const before = counts(store),
    repeated = await learnResearch(store, 'fixture-request');
  assert.equal(repeated.status, 'already_learned');
  assert.deepEqual(repeated.sources, result.sources);
  assert.deepEqual(counts(store), before);
});

test('independent instances replay receipts across restart without changing the common schema', async (t) => {
  const { store, settings } = fixture(t);
  const before = store.connection.query(
    "SELECT name,sql FROM sqlite_master WHERE name NOT GLOB 'kraken_*' AND name NOT GLOB 'idx_kraken_*' ORDER BY name",
  );
  seed(store);
  const learned = await learnResearch(store, 'fixture-request');
  const reopened = new KrakenStore(settings);
  try {
    assert.equal((await learnResearch(reopened, 'fixture-request')).status, 'already_learned');
    assert.equal(readResearch(reopened, 'fixture-request').reportHash, learned.reportHash);
    assert.deepEqual(
      reopened.connection.query(
        "SELECT name,sql FROM sqlite_master WHERE name NOT GLOB 'kraken_*' AND name NOT GLOB 'idx_kraken_*' ORDER BY name",
      ),
      before,
    );
    assert.equal(reopened.connection.query('PRAGMA integrity_check')[0].integrity_check, 'ok');
  } finally {
    reopened.close();
  }
});

test('project and agent scopes isolate requests and source evidence in one database', async (t) => {
  const { store, settings } = fixture(t);
  seed(store);
  const first = await learnResearch(store, 'fixture-request');
  for (const options of [{ project: 'other' }, { agent: 'other' }]) {
    const other = new KrakenStore({ ...settings, ...options });
    try {
      assert.equal(readResearch(other, 'fixture-request'), undefined);
      await assert.rejects(learnResearch(other, 'fixture-request'), /not found/);
      seed(other);
      const second = await learnResearch(other, 'fixture-request');
      assert.equal(
        second.sources.some((source) => first.sources.some((old) => old.id === source.id)),
        false,
      );
    } finally {
      other.close();
    }
  }
});

test('changed sources invalidate old evidence and stale reports cannot restore it', async (t) => {
  const { store } = fixture(t);
  seed(store);
  const first = await learnResearch(store, 'fixture-request');
  const changed = report(
    'Rust is a programming language with an updated memory safety documentation example.',
  );
  for (const page of changed.pages) page.fetched_at = '1700000001';
  seed(store, 'new-request', changed);
  store.connection.execute(
    'UPDATE kraken_research SET created_at=created_at+1000 WHERE request_id=?',
    ['new-request'],
  );
  const second = await learnResearch(store, 'new-request');
  assert.notEqual(second.sources[0].id, first.sources[0].id);
  assert.equal(
    store.connection.query('SELECT 1 FROM mimir_evidence_invalidations WHERE experience_id=?', [
      first.sources[0].id,
    ]).length,
    1,
  );
  const before = counts(store);
  await assert.rejects(learnResearch(store, 'fixture-request'), /older or conflicting observation/);
  assert.deepEqual(counts(store), before);
});

test('pruned or invalidated source evidence generates fresh sources and conclusions', async (t) => {
  const { store } = fixture(t);
  seed(store);
  const first = await learnResearch(store, 'fixture-request');
  store.memory.reasoning.invalidateEvidence(first.sources[0].id);
  const second = await learnResearch(store, 'fixture-request');
  assert.equal(second.status, 'learned');
  assert.notEqual(second.sources[0].id, first.sources[0].id);
  store.connection.execute('DELETE FROM experiences WHERE id=?', [second.sources[1].id]);
  const third = await learnResearch(store, 'fixture-request');
  assert.notEqual(third.sources[1].id, second.sources[1].id);
  assert.equal(store.memory.reasoning.latestConclusion(third.insights[0].id).isVerified, false);
});

test('a failure after source insertion rolls back every knowledge and receipt write', async (t) => {
  const { store } = fixture(t);
  seed(store);
  const before = counts(store),
    original = store.memory.insertInsight.bind(store.memory);
  store.memory.insertInsight = () => {
    throw new Error('injected failure');
  };
  await assert.rejects(learnResearch(store, 'fixture-request'), /injected failure/);
  assert.deepEqual(counts(store), before);
  store.memory.insertInsight = original;
  assert.equal((await learnResearch(store, 'fixture-request')).status, 'learned');
});

test('host policy may reject all candidates and cannot mutate or invent eligible evidence', async (t) => {
  const { store } = fixture(t);
  seed(store);
  const rejected = await learnResearch(store, 'fixture-request', {
    id: 'reject-v1',
    screen(input) {
      input.pages[0].content = 'altered';
      return [];
    },
  });
  assert.equal(rejected.sources.length, 0);
  assert.equal(rejected.insights.length, 0);
  const before = counts(store);
  for (const selected of [[299], [0, 0], ['0'], null])
    await assert.rejects(
      learnResearch(store, 'fixture-request', { id: 'invalid-v1', screen: () => selected }),
    );
  assert.deepEqual(counts(store), before);
  assert.equal(
    readResearch(store, 'fixture-request').report.pages[0].content,
    report().pages[0].content,
  );
});

test('policy cancellation leaves shared knowledge unchanged even when the callback ignores abort', async (t) => {
  const { store } = fixture(t);
  seed(store);
  const before = counts(store),
    controller = new AbortController();
  const learning = learnResearch(
    store,
    'fixture-request',
    { id: 'stalled-v1', screen: () => new Promise(() => {}) },
    controller.signal,
  );
  setTimeout(() => controller.abort(), 20);
  await assert.rejects(learning, /cancelled/);
  assert.deepEqual(counts(store), before);
});

test('concurrent matching learning requests converge on one receipt', async (t) => {
  const { store } = fixture(t);
  seed(store);
  const policy = {
    id: 'review-v1',
    async screen() {
      await new Promise((resolve) => setTimeout(resolve, 10));
      return [0];
    },
  };
  const results = await Promise.all([
    learnResearch(store, 'fixture-request', policy),
    learnResearch(store, 'fixture-request', policy),
  ]);
  assert.deepEqual(results[0].sources, results[1].sources);
  assert.deepEqual(counts(store), [3, 1, 1, 3, 1]);
});

test('foreign source pointers are rejected before any shared evidence is modified', async (t) => {
  const { store } = fixture(t);
  seed(store);
  const learned = await learnResearch(store, 'fixture-request');
  const foreign = store.memory.insertExperience({
    agent: 'someone',
    project: 'elsewhere',
    type: 'pattern',
    category: 'web_source',
    context: '{}',
    action: 'sentinel',
    outcome: 'preserve',
  });
  store.connection.execute('UPDATE kraken_sources SET experience_id=? WHERE url=?', [
    foreign.id,
    learned.sources[0].url,
  ]);
  const before = counts(store);
  await assert.rejects(learnResearch(store, 'fixture-request'));
  assert.equal(store.memory.getExperience(foreign.id).outcome, 'preserve');
  assert.equal(
    store.connection.query('SELECT 1 FROM mimir_evidence_invalidations WHERE experience_id=?', [
      foreign.id,
    ]).length,
    0,
  );
  assert.deepEqual(counts(store), before);
});

test('repeated research reuses the same claim and reports zero newly learned insights', async (t) => {
  const { store } = fixture(t);
  seed(store, 'first');
  const first = await learnResearch(store, 'first');
  seed(store, 'second');
  const second = await learnResearch(store, 'second');
  assert.equal(first.newSources, 3);
  assert.equal(first.newInsights, 1);
  assert.equal(second.newSources, 0);
  assert.equal(second.newInsights, 0);
  assert.deepEqual(second.sources, first.sources);
  assert.deepEqual(second.insights, first.insights);
  assert.deepEqual(counts(store), [3, 1, 2, 3, 2]);
});

test('repairing a receipt retains the original decision for the same policy version', async (t) => {
  const { store } = fixture(t);
  seed(store);
  const first = await learnResearch(store, 'fixture-request', {
    id: 'stable-v1',
    screen: () => [0],
  });
  store.memory.reasoning.invalidateEvidence(first.sources[0].id);
  let called = false;
  const repaired = await learnResearch(store, 'fixture-request', {
    id: 'stable-v1',
    screen: () => {
      called = true;
      return [];
    },
  });
  assert.equal(called, false);
  assert.deepEqual(repaired.accepted, [0]);
  assert.equal(repaired.newSources, 1);
  assert.equal(repaired.newInsights, 1);
});

test('custom screening cannot reuse the reserved default policy identifier', async (t) => {
  const { store } = fixture(t);
  seed(store);
  await learnResearch(store, 'fixture-request');
  await assert.rejects(
    learnResearch(store, 'fixture-request', { id: 'source-agreement-v1', screen: () => [] }),
    /own versioned/,
  );
});

test('late-finishing old research cannot replace a more recently retrieved source', async (t) => {
  const { store } = fixture(t);
  seed(store, 'old');
  const latest = report(
    'Rust is a language whose updated documentation describes reliable memory ownership.',
  );
  for (const page of latest.pages) page.fetched_at = '1700000001';
  seed(store, 'new', latest);
  const current = await learnResearch(store, 'new');
  store.connection.execute(
    'UPDATE kraken_research SET created_at=created_at+100000 WHERE request_id=?',
    ['old'],
  );
  await assert.rejects(learnResearch(store, 'old'), /older or conflicting observation/);
  const repeated = await learnResearch(store, 'new');
  assert.deepEqual(repeated.sources, current.sources);
});
