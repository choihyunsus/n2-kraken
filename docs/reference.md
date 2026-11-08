[Back to the getting-started guide](../README.md)

# N2 Kraken

[한국어](reference.ko.md)

Public web retrieval and source-backed research for local agents. Kraken provides an HTML reader, bounded HTTP fetching, installed-Chrome rendering, search-engine fallbacks, claim comparison, and explicit learning through the shared Soul/Mimir SQLite database.

## Install

Requires Node.js 22 or newer. Install the GitHub release archive.

The archive includes workers for Windows x64, Linux x64 and macOS arm64. The Linux worker requires glibc 2.39 or newer and the system C++ runtime. The macOS worker declares a deployment target of 11.0, and the installed Node.js version has its own OS requirements. CI exercises the current GitHub-hosted runner for each platform. Build locally for other architectures or older compatible systems.

```sh
npm install https://github.com/choihyunsus/n2-kraken/releases/download/v1.0.0/n2-kraken-1.0.0.tgz
```

The native worker is selected from `bin/n2-kraken-worker-<platform>-<architecture>`. Windows workers have an `.exe` suffix. An explicit `workerPath` selects an operator-built executable. If your platform needs a local build, install Rust 1.98.0 through rustup, a C/C++ toolchain, CMake and LLVM/libclang, then run the following inside the installed package.

```sh
npm run build:native
```

Chrome is required for rendering and Google search. Configure `chromePath` when automatic discovery cannot locate it. Optional semantic comparison uses an already available Ollama embedding model.

## Library

```js
const { Kraken } = require('n2-kraken');

const kraken = new Kraken({
  dbPath: './data/mimir.db',
  project: 'my-project',
  agent: 'researcher',
});

try {
  const page = await kraken.fetch({ url: 'https://example.com/' });
  console.log(page.source_url, page.markdown, page.truncated);

  const research = await kraken.research({
    requestId: 'rust-memory-safety-001',
    topic: 'Rust memory safety borrowing',
    config: { max_results: 5, max_crawl_pages: 3 },
  });
  console.log(research.report.summary);

  // Explicitly publish eligible source agreement as unverified knowledge.
  const learned = await kraken.learn(research.requestId);
  console.log(learned.newInsights, learned.verified);
} finally {
  await kraken.close();
}
```

The ESM entry point supports `import { Kraken } from 'n2-kraken'`. Type declarations are included. Every instance requires explicit `dbPath`, `project` and `agent` values. Passing Soul's existing `mimir.db` shares the common experience, insight, tag and reasoning contracts. Each instance operates in its configured project and agent scope.

### Operations

| Method | Result |
| --- | --- |
| `extract({html, source_url?, config?}, signal?)` | Markdown, source identity and truncation metadata |
| `fetch({url, config?}, signal?)` | HTTP retrieval with final redirect URL |
| `render({url, format?, max_length?}, signal?)` | Live-DOM text or rendered HTML |
| `search({query, config?}, signal?)` | Deduplicated search results |
| `research({requestId, topic, config?, semantic?}, signal?)` | Persisted summary, source pages, failures and agreement candidates |
| `getResearch(requestId)` | The full stored receipt, or `undefined` |
| `listResearch(limit?, offset?)` | Scoped receipt identifiers and hashes |
| `getLearning(requestId, policyId?)` | Historical learning receipt without running a worker or policy |
| `learn(requestId, {policy?, signal?}?)` | Atomic evidence, insight and learning receipt publication |
| `close()` | Cancellation of owned work followed by database closure |

`extract` and `fetch` accept `include_links`, `include_images` and UTF-8 byte `max_length` options in `config`. The default output limit is 50,000 bytes. `render` defaults to `format: 'text'` and the same byte limit.

### Research configuration

| Field | Default | Supported range |
| --- | --- | --- |
| `max_results` | 10 | 1–40 |
| `max_crawl_pages` | 5 | 1–20, applied to the returned search results |
| `max_content_length` | 5,000 | 50–100,000 UTF-8 bytes per source |
| `min_confidence` | 0.6 | 0.5–1.0 heuristic agreement |
| `timeout_ms` | 30,000 | 100–120,000 per engine/page |
| `total_timeout_ms` | 180,000 | 100–600,000 for the operation |
| `time_range` | `3m` | empty, `1w`, `1m`, `3m`, `6m`, `1y` |
| `engines` | `ddg,bing,naver` | Unique ordered subset including optional `google,searxng` |

Search stops at the first engine returning usable results. Date filtering follows each engine's supported granularity. Search markup, ranking and website access conditions can change. Google can return an interactive verification challenge, which Kraken reports as a retrieval failure.

Configure operator-owned services on the instance.

```js
const kraken = new Kraken({
  dbPath: './data/mimir.db', project: 'my-project', agent: 'researcher',
  searxngUrl: 'http://127.0.0.1:8080/',
  ollama: { endpoint: 'http://127.0.0.1:11434', model: 'nomic-embed-text' },
});
```

Select `engines: ['searxng']` to use the configured search service. Set `semantic: true` on research to request embeddings. The default semantic service is local Ollama with `nomic-embed-text`. Unavailable, invalid or oversized embedding results fall back to keyword matching, and `summary.semantic_mode` records the actual method used.

## Learning and provenance

Each research request stores the native worker hash, resolved-input hash, report hash, exact source text hashes, final URLs, truncation flags and retrieval failures. A repeated request ID with matching inputs returns its stored result. Reusing an ID with different inputs raises an error. Use a new ID for a fresh observation.

The native `Verified` agreement status requires at least three distinct source URLs, the configured score threshold and no detected contradiction. These checks measure agreement. Factual accuracy, publisher independence, copied text and sophisticated misinformation require additional evaluation. The keyword and contradiction rules have limited linguistic coverage and may reject legitimate technical text.

`learn` stores source content as `pattern` experiences and creates source-linked, unverified reasoning conclusions. It deduplicates equivalent claims over the same evidence across requests. New source versions invalidate prior evidence atomically. Conflicting older reports cannot restore superseded content. A missing or pruned source requires a fresh evidence generation before receipt reuse.

A host policy can further narrow eligible candidates. Give each policy revision a stable, distinct ID. Its prior accepted selection is retained when repairing a receipt for the same policy ID.

```js
await kraken.learn('rust-memory-safety-001', {
  policy: {
    id: 'review-v1',
    async screen({ facts, pages }, signal) {
      signal.throwIfAborted();
      return facts.filter(({ fact }) =>
        fact.sources.every(url => pages.some(page => page.url === url && !page.truncated))
      ).map(({ index }) => index);
    },
  },
});
```

The asynchronous policy deadline is 30 seconds. Synchronous host callbacks must return promptly. Publication uses one Mimir transaction with a 15-second bound. `newSources` and `newInsights` describe additions made by the current call. The module records its schema in `meta.kraken_schema_version` and owns four scoped tables, `kraken_research`, `kraken_sources`, `kraken_claims` and `kraken_learning`. Raw reports have a 4 MiB per-record limit. Retained history grows with use and remains available for operator-managed archival.

Each source and insight reference also retains a durable `created` flag for the transaction that wrote that receipt generation. A consumer recovering after a lost response can distinguish its newly created knowledge from reused evidence. Legacy receipts without that field return `null`, preserving unknown creation history.

`getLearning` reads that historical receipt, including custom policy versions, after restart. It performs no repair or policy callback. Later source invalidation can make historical evidence stale, so the receipt describes the recorded transaction.

## CLI and MCP

```sh
n2-kraken --help
n2-kraken extract --db ./data/mimir.db --project demo --agent reader < request.json
n2-kraken get --db ./data/mimir.db --project demo --agent reader --id research-001
n2-kraken learn --db ./data/mimir.db --project demo --agent reader --id research-001
n2-kraken mcp --db ./data/mimir.db --project demo --agent reader
```

The CLI also accepts `--input` JSON, `--worker`, `--chrome`, `--searxng`, `--ollama-endpoint` and `--ollama-model`. `history` accepts `--limit` and `--offset`. Successful commands write JSON to stdout. Failures write a bounded error to stderr and return exit code 1.

The stdio MCP server exposes ten tools, `kraken_extract`, `kraken_fetch`, `kraken_render`, `kraken_search`, `kraken_research`, `kraken_history`, `kraken_read`, `kraken_source`, `kraken_facts` and `kraken_learn`. Responses are bounded to 60,000 bytes. Source text uses character-based pagination and facts reference source indices. Retrieval previews disclose truncation. `kraken_read` includes up to five retrieval failures, while the library and CLI expose the full receipt.

## Execution boundaries

Public page retrieval validates destinations and pins resolved public IP addresses, including redirects. The Chrome instance uses its own temporary profile and a constrained public-network proxy. Explicit SearXNG and Ollama endpoints are trusted host configuration and may use local addresses. Configure access controls around those services in the host application.

The worker protocol permits 16 MiB requests and 32 MiB responses. At most two workers run concurrently per Node.js process. HTTP bodies are limited to 8 MiB, rendered HTML to two million UTF-16 code units, and DOM extraction reports clipping and omitted frames. Source hashes describe the returned content. Website scripts run in installed Chrome with its sandbox enabled. Browser process memory varies with page behavior. Cancellation and shutdown target the processes launched by this instance. Forced host termination can leave temporary files.

## Build and verify

```sh
npm ci
npm run build:native
npm test
cargo +1.98.0 test --manifest-path native/Cargo.toml --all-targets -j 2
cargo +1.98.0 clippy --manifest-path native/Cargo.toml --all-targets -j 2 -- -D warnings
```

Tests include native worker RPC, configured SearXNG transport, public-destination rejection, shared SQLite transactions, restart and retry, cross-scope isolation, invalidated evidence, Unicode clipping, cancellation, CLI and actual stdio MCP. Live website and installed-Chrome checks are explicit ignored Rust tests, allowing deterministic CI to run without depending on search-engine availability.

## License

Kraken's module code is MIT. The Node.js facade depends on N2 Mimir, whose distributed license includes non-commercial terms and a separate commercial license. Review the bundled `LICENSE-N2-MIMIR`, `LICENSE-APACHE`, `THIRD-PARTY-NOTICES` and native dependency notices for distribution and use conditions. The native crate's third-party dependencies retain their respective licenses.
