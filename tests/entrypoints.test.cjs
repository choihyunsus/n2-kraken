// Exercise the public CLI and actual stdio MCP transport against a shared persisted fixture.
const test = require('node:test');
const assert = require('node:assert/strict');
const path = require('node:path');
const { spawnSync } = require('node:child_process');
const { Client } = require('@modelcontextprotocol/sdk/client/index.js');
const { StdioClientTransport } = require('@modelcontextprotocol/sdk/client/stdio.js');
const { fixture, seed, workerPath } = require('./helpers.cjs');
const { argumentsForCli } = require('../dist/cli-options');
const entry = path.resolve(__dirname, '../dist/index.js');

function options(settings) {
  return [
    '--db',
    settings.dbPath,
    '--project',
    settings.project,
    '--agent',
    settings.agent,
    '--worker',
    workerPath,
  ];
}
function run(args, input) {
  const result = spawnSync(process.execPath, [entry, ...args], {
    input,
    encoding: 'utf8',
    timeout: 15000,
    windowsHide: true,
  });
  assert.equal(result.error, undefined);
  return result;
}

test('CLI extraction, persisted research inspection and learning use normal entry points', (t) => {
  const { store, settings } = fixture(t);
  seed(store);
  const flags = options(settings);
  assert.match(run(['--help']).stdout, /research/);
  assert.equal(run(['--version']).stdout.trim(), '1.0.0');
  const extracted = run(
    ['extract', ...flags],
    JSON.stringify({ html: '<main>CLI fixture 한글</main>' }),
  );
  assert.equal(extracted.status, 0, extracted.stderr);
  assert.equal(JSON.parse(extracted.stdout).markdown, 'CLI fixture 한글');
  const learned = run(['learn', ...flags, '--id', 'fixture-request']);
  assert.equal(learned.status, 0, learned.stderr);
  assert.equal(JSON.parse(learned.stdout).newInsights, 1);
  assert.equal(
    JSON.parse(run(['learn', ...flags, '--id', 'fixture-request']).stdout).status,
    'already_learned',
  );
  assert.equal(JSON.parse(run(['history', ...flags]).stdout).length, 1);
  assert.equal(
    JSON.parse(run(['get', ...flags, '--id', 'fixture-request']).stdout).report.pages.length,
    3,
  );
  assert.equal(run(['get', ...flags, '--id', 'missing']).status, 1);
  assert.equal(run(['extract', ...flags], '{').status, 1);
});

test('CLI rejects duplicate, unknown and incompatible options', () => {
  for (const args of [
    ['history', '--db', 'one', '--db', 'two'],
    ['history', '--unknown'],
    ['learn', '--input', '{}'],
    ['extract', '--id', 'other'],
    ['get', '--limit', '1'],
  ])
    assert.throws(() => argumentsForCli(args));
  assert.equal(run(['history']).status, 1);
  assert.equal(run(['unknown']).status, 1);
});

test('real stdio MCP lists ten tools and reads, learns and extracts through the executable', async (t) => {
  const { store, settings } = fixture(t);
  seed(store);
  const transport = new StdioClientTransport({
    command: process.execPath,
    args: [entry, 'mcp', ...options(settings)],
    stderr: 'pipe',
  });
  const client = new Client({ name: 'kraken-fixture', version: '1.0.0' });
  let diagnostics = '';
  transport.stderr.on('data', (chunk) => {
    diagnostics += chunk;
  });
  try {
    await client.connect(transport);
    assert.equal((await client.listTools()).tools.length, 10);
    const call = async (name, args) => {
      const response = await client.callTool({ name, arguments: args });
      assert.notEqual(response.isError, true, JSON.stringify(response));
      assert.ok(Buffer.byteLength(response.content[0].text) <= 60000);
      return JSON.parse(response.content[0].text);
    };
    assert.equal((await call('kraken_history', {})).length, 1);
    assert.equal((await call('kraken_read', { requestId: 'fixture-request' })).sourceCount, 3);
    const page = await call('kraken_source', { requestId: 'fixture-request', index: 0, limit: 10 });
    assert.equal(Array.from(page.content).length, 10);
    assert.equal(page.nextOffset, 10);
    const facts = await call('kraken_facts', { requestId: 'fixture-request' });
    assert.deepEqual(facts.facts[0].sourceIndices, [0, 1, 2]);
    assert.equal(facts.facts[0].verified, false);
    assert.equal((await call('kraken_learn', { requestId: 'fixture-request' })).newInsights, 1);
    assert.equal(
      (await call('kraken_extract', { html: '<main>MCP fixture</main>' })).content,
      'MCP fixture',
    );
    const failure = await client.callTool({
      name: 'kraken_read',
      arguments: { requestId: 'missing' },
    });
    assert.equal(failure.isError, true);
  } finally {
    await client.close();
  }
  assert.equal(diagnostics, '');
});
