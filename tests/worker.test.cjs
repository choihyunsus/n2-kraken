// Exercise the real native entry point and bounded child transport without external services.
const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const http = require('node:http');
const { spawn, spawnSync } = require('node:child_process');
const { once } = require('node:events');
const { Kraken } = require('../dist/library');
const { collectWorker } = require('../dist/worker-process');
const { fixture, workerPath } = require('./helpers.cjs');

function fake(code) {
  return spawn(process.execPath, ['-e', `process.stdin.resume();${code}`], {
    stdio: ['pipe', 'pipe', 'pipe'],
    detached: process.platform !== 'win32',
    windowsHide: true,
  });
}
async function service(t, handler) {
  const server = http.createServer(handler).listen(0, '127.0.0.1');
  await once(server, 'listening');
  t.after(() => {
    server.closeAllConnections();
    server.close();
  });
  return `http://127.0.0.1:${server.address().port}/prefix`;
}

test('real worker parses HTML with source identity and byte-safe truncation', async (t) => {
  assert.ok(fs.existsSync(workerPath), 'Build the native worker before running the tests');
  const { settings } = fixture(t),
    engine = new Kraken({ ...settings, workerPath });
  try {
    const result = await engine.extract({
      html: '<title>한글 😀</title><main><p>Hello 한글 😀</p></main>',
      source_url: 'https://example.com/source#fragment',
      config: { max_length: 11 },
    });
    assert.equal(result.title, '한글 😀');
    assert.equal(result.source_url, 'https://example.com/source');
    assert.equal(result.truncated, true);
    assert.ok(Buffer.byteLength(result.markdown) <= 11);
    assert.equal(result.markdown.includes('\ufffd'), false);
  } finally {
    await engine.close();
  }
  assert.throws(() => engine.getResearch('anything'), /closed/);
});

test('native protocol rejects unknown fields and unsupported operations before execution', () => {
  for (const command of [
    { operation: 'unsupported', arguments: {} },
    { operation: 'extract', arguments: { html: '', extra: true } },
  ]) {
    const result = spawnSync(workerPath, [], {
      input: JSON.stringify({ protocol: 1, id: 'fixture', command }),
      encoding: 'utf8',
    });
    assert.equal(result.status, 2);
    assert.equal(JSON.parse(result.stdout).error.code, 'invalid_request');
  }
  const version = spawnSync(workerPath, ['--version'], { encoding: 'utf8' });
  assert.equal(version.status, 0);
  assert.match(version.stdout, /^n2-kraken-worker 1\.0\.0/);
});

test('configured SearXNG is actually queried while its private crawl targets remain blocked', async (t) => {
  let searches = 0,
    unexpected = 0;
  const searxngUrl = await service(t, (request, response) => {
    const url = new URL(request.url, 'http://fixture');
    if (url.pathname !== '/prefix/search') {
      unexpected++;
      response.end('private sentinel');
      return;
    }
    searches++;
    assert.equal(url.searchParams.get('q'), 'Rust memory safety &url=https://other.invalid');
    response.setHeader('Content-Type', 'application/json');
    response.end(
      JSON.stringify({
        results: [
          {
            title: 'Private fixture',
            url: searxngUrl + '/private',
            content: 'Must never be crawled',
          },
        ],
      }),
    );
  });
  const { settings } = fixture(t),
    engine = new Kraken({ ...settings, workerPath, searxngUrl });
  try {
    const input = {
      requestId: 'native-fixture',
      topic: 'Rust memory safety &url=https://other.invalid',
      config: { engines: ['searxng'], max_results: 1, max_crawl_pages: 1, total_timeout_ms: 5000 },
    };
    const first = await engine.research(input);
    assert.equal(first.report.summary.search_results, 1);
    assert.equal(first.report.summary.pages_crawled, 0);
    assert.equal(first.report.failures.length, 1);
    assert.equal(searches, 1);
    assert.equal(unexpected, 0);
    const second = await engine.research(input);
    assert.equal(second.reportHash, first.reportHash);
    assert.equal(searches, 1);
    const learned = await engine.learn(input.requestId);
    assert.equal(learned.insights.length, 0);
    assert.equal(engine.listResearch().length, 1);
  } finally {
    await engine.close();
  }
});

test('fetch and render reject loopback destinations before the target receives a request', async (t) => {
  let hits = 0;
  const url = await service(t, (_, response) => {
    hits++;
    response.end('private');
  });
  const { settings } = fixture(t),
    engine = new Kraken({ ...settings, workerPath });
  try {
    await assert.rejects(engine.fetch({ url }));
    await assert.rejects(engine.render({ url }));
    assert.equal(hits, 0);
  } finally {
    await engine.close();
  }
});

test('close cancels an outstanding native request and settles before database shutdown', async (t) => {
  const searxngUrl = await service(t, () => {});
  const { settings } = fixture(t),
    engine = new Kraken({ ...settings, workerPath, searxngUrl });
  const pending = engine.search({ query: 'stalled service', config: { engines: ['searxng'] } });
  const rejected = assert.rejects(pending, /cancelled/);
  await new Promise((resolve) => setTimeout(resolve, 100));
  await engine.close();
  await rejected;
  await engine.close();
});

test('child transport rejects malformed UTF-8 and oversized standard output', async () => {
  await assert.rejects(
    collectWorker(fake('process.stdout.write(Buffer.from([0xff]));process.exit(0)'), '{}', 5000),
    /UTF-8/,
  );
  await assert.rejects(
    collectWorker(fake('process.stdout.write(Buffer.alloc(33*1048576));'), '{}', 5000),
    /byte limit/,
  );
});

test('child transport bounds diagnostic output and stops deadline or cancellation failures', async () => {
  await assert.rejects(
    collectWorker(fake('process.stderr.write(Buffer.alloc(2*1048576));'), '{}', 5000),
    /diagnostic/,
  );
  await assert.rejects(collectWorker(fake('setInterval(()=>{},1000)'), '{}', 300), /deadline/);
  const controller = new AbortController();
  const pending = collectWorker(fake('setInterval(()=>{},1000)'), '{}', 5000, controller.signal);
  setTimeout(() => controller.abort(), 50);
  await assert.rejects(pending, /cancelled/);
});
