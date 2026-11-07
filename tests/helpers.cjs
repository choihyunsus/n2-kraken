// Private per-test databases and deterministic source documents exercise shared storage behavior.
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { digest } = require('../dist/values');
const { KrakenStore } = require('../dist/store');
const { storeResearch } = require('../dist/research-store');

const suffix = process.platform === 'win32' ? '.exe' : '';
const packagedWorker = path.resolve(
  __dirname,
  '../bin',
  `n2-kraken-worker-${process.platform}-${process.arch}${suffix}`,
);
const workerPath =
  process.env.N2_KRAKEN_TEST_WORKER ||
  (fs.existsSync(packagedWorker)
    ? packagedWorker
    : path.resolve(__dirname, '../native/target/debug', `n2-kraken-worker${suffix}`));
function fixture(t, options = {}) {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'kraken-test-'));
  const settings = {
    dbPath: path.join(directory, 'mimir.db'),
    project: 'fixture',
    agent: 'tester',
    ...options,
  };
  const store = new KrakenStore(settings);
  t.after(() => {
    store.close();
    const resolved = fs.realpathSync.native(directory),
      root = fs.realpathSync.native(os.tmpdir());
    if (path.dirname(resolved) !== root || !path.basename(resolved).startsWith('kraken-test-'))
      throw new Error('Fixture directory ownership changed');
    fs.rmSync(resolved, { recursive: true, force: true });
  });
  return { store, settings, directory };
}
function report(
  text = 'Rust is a programming language with memory safety and reliable concurrency.',
) {
  const pages = ['one', 'two', 'three'].map((name) => ({
    url: `https://${name}.example/article`,
    title: 'Source fixture',
    content: text,
    content_length: Buffer.byteLength(text),
    content_hash: digest(text),
    fetched_at: '1700000000',
    truncated: false,
  }));
  return {
    summary: {
      topic: 'Rust memory safety',
      semantic_mode: false,
      search_results: 3,
      pages_crawled: 3,
      claims_extracted: 3,
      facts_verified: 1,
      facts_pending: 0,
      facts_rejected: 0,
    },
    pages,
    failures: [],
    facts: [
      {
        claim: text,
        confidence: 1,
        sources: pages.map((page) => page.url),
        status: 'Verified',
        contradiction: null,
      },
    ],
  };
}
function seed(store, requestId = 'fixture-request', data = report()) {
  return storeResearch(store, requestId, digest(requestId), digest('fixture-worker'), data);
}
function counts(store) {
  return ['experiences', 'insights', 'kraken_research', 'kraken_sources', 'kraken_learning'].map(
    (name) => store.connection.query(`SELECT count(*) AS count FROM ${name}`)[0].count,
  );
}
module.exports = { fixture, report, seed, counts, workerPath };
