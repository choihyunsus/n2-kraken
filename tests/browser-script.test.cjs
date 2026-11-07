// DOM fixtures catch UTF-16 boundary corruption and extraction-coverage omissions without Chrome.
const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const source = fs.readFileSync(path.join(__dirname, '../native/src/scripts/extract.js'), 'utf8');

function extract(texts, title = '', location = 'https://example.com/') {
  const root = {
    tagName: 'MAIN',
    parentElement: null,
    getAttribute() {
      return null;
    },
  };
  const parent = {
    tagName: 'P',
    parentElement: root,
    getAttribute() {
      return null;
    },
  };
  let index = 0;
  const context = {
    document: {
      title,
      querySelector: () => root,
      createTreeWalker: () => ({
        nextNode: () =>
          index < texts.length ? { textContent: texts[index++], parentElement: parent } : null,
      }),
    },
    NodeFilter: { SHOW_TEXT: 4 },
    location: { href: location },
    URL,
    window: { getComputedStyle: () => ({ display: 'block', visibility: 'visible', opacity: '1' }) },
  };
  return JSON.parse(vm.runInNewContext(source, context, { timeout: 1000 }));
}

test('emoji at text and title boundaries never produces a lone UTF-16 surrogate', () => {
  const result = extract(['x'.repeat(3999) + '😀'], 'x'.repeat(8191) + '😀');
  assert.equal(result.truncated, true);
  assert.equal(result.content, 'x'.repeat(3999));
  assert.equal(result.title, 'x'.repeat(8191));
  assert.equal(Buffer.from(result.content).toString('utf8'), result.content);
});

test('line bounds report actual omissions while exact boundaries remain complete', () => {
  assert.equal(extract(Array(800).fill('text')).truncated, false);
  assert.equal(extract(Array(801).fill('text')).truncated, true);
  assert.equal(extract(['unpaired\ud800']).content, 'unpaired\ufffd');
  assert.throws(
    () => extract(['text'], '', 'https://example.com/' + 'x'.repeat(8192)),
    /URL exceeds/,
  );
});
