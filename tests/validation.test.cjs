// Boundary tests reject ambiguous identifiers, source mutations and unsupported schemas.
const test = require('node:test');
const assert = require('node:assert/strict');
const { fixture, report, seed } = require('./helpers.cjs');
const { researchReport } = require('../dist/report');
const { researchInput, extractInput } = require('../dist/input');
const { readResearch, storeResearch, listResearch } = require('../dist/research-store');
const { digest } = require('../dist/values');
const { initializeSchema } = require('../dist/schema');

test('native reports reject hash, source, agreement, contradiction and summary corruption', () => {
  const changes = [
    (data) => (data.pages[0].content += 'corruption'),
    (data) => data.pages[0].content_length++,
    (data) => (data.pages[0].url = data.pages[1].url),
    (data) => data.facts[0].sources.pop(),
    (data) => data.facts[0].sources.push('https://unknown.example/'),
    (data) => (data.facts[0].contradiction = 'conflict'),
    (data) => (data.facts[0].claim = 'This statement never appeared in any retrieved source.'),
    (data) => data.summary.facts_verified++,
  ];
  for (const change of changes) {
    const data = report();
    change(data);
    assert.throws(() => researchReport.parse(data));
  }
  assert.deepEqual(researchReport.parse(report()), report());
});

test('inputs reject malformed Unicode and callers cannot supply trusted network endpoints', () => {
  for (const requestId of ['', 'with space', 'bad\0id', '\ud800', '\u0085', '한'.repeat(43)])
    assert.throws(() => researchInput.parse({ requestId, topic: 'Rust' }));
  assert.throws(() =>
    researchInput.parse({
      requestId: 'valid',
      topic: 'Rust',
      config: { searxng_url: 'http://localhost/' },
    }),
  );
  for (const source_url of [
    'file:///etc/passwd',
    'https://name:secret@example.com/',
    'https://example.com:0/',
    'https://example.com/\n',
  ])
    assert.throws(() => extractInput.parse({ html: '', source_url }));
  assert.throws(() => extractInput.parse({ html: '\ud800' }));
});

test('research retries reject input conflicts and stored digest corruption', (t) => {
  const { store } = fixture(t),
    first = seed(store);
  assert.equal(
    storeResearch(store, first.requestId, first.inputHash, first.workerHash, report()).reportHash,
    first.reportHash,
  );
  assert.throws(
    () => storeResearch(store, first.requestId, digest('other'), first.workerHash, report()),
    /different inputs/,
  );
  assert.equal(listResearch(store).length, 1);
  assert.throws(() => listResearch(store, 101));
  store.connection.execute('UPDATE kraken_research SET report_json=?', ['{}']);
  assert.throws(() => readResearch(store, first.requestId), /digest/);
});

test('future and unversioned schemas require an explicit migration', (t) => {
  const { store } = fixture(t);
  store.connection.execute("UPDATE meta SET value='2' WHERE key='kraken_schema_version'");
  assert.throws(() => initializeSchema(store.connection), /Unsupported/);
  store.connection.execute("DELETE FROM meta WHERE key='kraken_schema_version'");
  assert.throws(() => initializeSchema(store.connection), /Unversioned/);
});
