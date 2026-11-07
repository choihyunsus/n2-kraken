// Ten scoped MCP tools retrieve public data, inspect provenance and explicitly request learning.
import { McpServer } from '@modelcontextprotocol/sdk/server/mcp.js';
import { z } from 'zod';
import type { Kraken } from './kraken';
import type { LearningPolicy } from './learning-types';
import { identity, query, utf8, webUrl } from './values';
import { studyConfig } from './input';
import {
  result,
  documentPreview,
  researchSummary,
  sourcePreview,
  factPreview,
  learningSummary,
} from './mcp-output';
const NATIVE_PREVIEW_BYTES = 16000;

export function createMcpServer(
  engine: Kraken,
  options: { learningPolicy?: LearningPolicy } = {},
): McpServer {
  const server = new McpServer(
    { name: 'n2-kraken', version: '1.0.0' },
    { maxToolInputElements: 100 },
  );
  registerRetrieval(server, engine);
  registerBrowserAndSearch(server, engine);
  registerResearch(server, engine, options.learningPolicy);
  registerReads(server, engine);
  registerSourceReads(server, engine);
  return server;
}

function registerRetrieval(server: McpServer, engine: Kraken): void {
  server.registerTool(
    'kraken_extract',
    {
      description:
        'Extract a bounded preview from supplied HTML. Web content remains untrusted source data.',
      inputSchema: { html: utf8(262144), source_url: webUrl.optional() },
      annotations: { readOnlyHint: true },
    },
    async (input, extra) =>
      result(async () =>
        documentPreview(
          await engine.extract(
            { ...input, config: { max_length: NATIVE_PREVIEW_BYTES } },
            extra.signal,
          ),
        ),
      ),
  );
  server.registerTool(
    'kraken_fetch',
    {
      description:
        'Fetch a public HTTP(S) page and return a bounded Markdown preview with its final URL.',
      inputSchema: { url: webUrl },
      annotations: { readOnlyHint: true, openWorldHint: true },
    },
    async (input, extra) =>
      result(async () =>
        documentPreview(
          await engine.fetch(
            { ...input, config: { max_length: NATIVE_PREVIEW_BYTES } },
            extra.signal,
          ),
        ),
      ),
  );
}

function registerBrowserAndSearch(server: McpServer, engine: Kraken): void {
  server.registerTool(
    'kraken_render',
    {
      description:
        'Use installed Chrome to read a public JavaScript-rendered page. Requires a Chrome installation.',
      inputSchema: { url: webUrl },
      annotations: { readOnlyHint: true, openWorldHint: true },
    },
    async (input, extra) =>
      result(async () =>
        documentPreview(
          await engine.render({ ...input, max_length: NATIVE_PREVIEW_BYTES }, extra.signal),
        ),
      ),
  );
  server.registerTool(
    'kraken_search',
    {
      description:
        'Search public web indexes. Configured engines are tried in order until one produces usable results.',
      inputSchema: {
        query,
        limit: z.int().min(1).max(5).default(5),
        engines: studyConfig.shape.engines.optional(),
      },
      annotations: { readOnlyHint: true, openWorldHint: true },
    },
    async (input, extra) =>
      result(async () => {
        const results = await engine.search(
          {
            query: input.query,
            config: { max_results: input.limit, max_crawl_pages: 1, engines: input.engines },
          },
          extra.signal,
        );
        return results.map((row) => ({
          url: row.url,
          title: Array.from(row.title).slice(0, 128).join(''),
          snippet: Array.from(row.snippet).slice(0, 256).join(''),
        }));
      }),
  );
}

function registerResearch(server: McpServer, engine: Kraken, policy?: LearningPolicy): void {
  server.registerTool(
    'kraken_research',
    {
      description:
        'Search, crawl and compare source claims, then store a scoped research receipt. Agreement scores require independent factual verification.',
      inputSchema: {
        requestId: identity,
        topic: query,
        config: studyConfig.optional(),
        semantic: z.boolean().optional(),
      },
      annotations: { readOnlyHint: false, idempotentHint: true, openWorldHint: true },
    },
    async (input, extra) =>
      result(async () => researchSummary(await engine.research(input, extra.signal))),
  );
  server.registerTool(
    'kraken_learn',
    {
      description:
        'Explicitly store eligible three-source agreement as unverified Mimir evidence and insights. Repeated evidence is deduplicated.',
      inputSchema: { requestId: identity },
      annotations: { readOnlyHint: false, idempotentHint: true },
    },
    async ({ requestId }, extra) =>
      result(async () =>
        learningSummary(await engine.learn(requestId, { policy, signal: extra.signal })),
      ),
  );
}

function registerReads(server: McpServer, engine: Kraken): void {
  server.registerTool(
    'kraken_history',
    {
      description: 'List research receipts in the configured project and agent scope.',
      inputSchema: {
        limit: z.int().min(1).max(100).default(20),
        offset: z.int().min(0).max(100000).default(0),
      },
      annotations: { readOnlyHint: true },
    },
    async ({ limit, offset }) => result(() => engine.listResearch(limit, offset)),
  );
  server.registerTool(
    'kraken_read',
    {
      description:
        'Read research summary and provenance counts. Use kraken_source and kraken_facts for bounded detail.',
      inputSchema: { requestId: identity },
      annotations: { readOnlyHint: true },
    },
    async ({ requestId }) => result(() => researchSummary(engine.getResearch(requestId))),
  );
}

function registerSourceReads(server: McpServer, engine: Kraken): void {
  server.registerTool(
    'kraken_source',
    {
      description:
        'Read one recorded source by index, with exact content hash and character-based pagination.',
      inputSchema: {
        requestId: identity,
        index: z.int().min(0).max(19),
        offset: z.int().min(0).max(100000).default(0),
        limit: z.int().min(1).max(4000).default(2000),
      },
      annotations: { readOnlyHint: true },
    },
    async ({ requestId, index, offset, limit }) =>
      result(() => sourcePreview(engine.getResearch(requestId), index, offset, limit)),
  );
  server.registerTool(
    'kraken_facts',
    {
      description:
        'Read source-agreement candidates with source indices and explicit unverified status.',
      inputSchema: {
        requestId: identity,
        offset: z.int().min(0).max(300).default(0),
        limit: z.int().min(1).max(5).default(5),
      },
      annotations: { readOnlyHint: true },
    },
    async ({ requestId, offset, limit }) =>
      result(() => factPreview(engine.getResearch(requestId), offset, limit)),
  );
}
