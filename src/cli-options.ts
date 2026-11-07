// Explicit CLI scope and host endpoints are kept separate from operation JSON input.
import { parseArgs } from 'node:util';
import type { KrakenOptions } from './kraken';
import { identity, MIB, utf8 } from './values';

export function argumentsForCli(args: string[]) {
  const parsed = parseArgs({
    args,
    allowPositionals: true,
    strict: true,
    tokens: true,
    options: {
      help: { type: 'boolean', short: 'h' },
      version: { type: 'boolean' },
      db: { type: 'string' },
      project: { type: 'string' },
      agent: { type: 'string' },
      worker: { type: 'string' },
      chrome: { type: 'string' },
      searxng: { type: 'string' },
      'ollama-endpoint': { type: 'string' },
      'ollama-model': { type: 'string' },
      input: { type: 'string' },
      id: { type: 'string' },
      limit: { type: 'string' },
      offset: { type: 'string' },
    },
  });
  const seen = new Set<string>();
  for (const token of parsed.tokens) {
    if (token.kind !== 'option') continue;
    if (seen.has(token.name)) throw new Error(`Duplicate option --${token.name}`);
    seen.add(token.name);
  }
  const command = parsed.positionals[0];
  if (parsed.values.id !== undefined && command !== 'learn' && command !== 'get')
    throw new Error('--id is supported by learn and get');
  if (
    (parsed.values.limit !== undefined || parsed.values.offset !== undefined) &&
    command !== 'history'
  )
    throw new Error('--limit and --offset are supported by history');
  if (
    parsed.values.input !== undefined &&
    !['extract', 'fetch', 'render', 'search', 'research'].includes(command ?? '')
  )
    throw new Error('--input requires a retrieval or research operation');
  return parsed;
}
export type CliArguments = ReturnType<typeof argumentsForCli>;

export function engineOptions(values: CliArguments['values']): KrakenOptions {
  const ollama =
    values['ollama-endpoint'] || values['ollama-model']
      ? {
          endpoint: values['ollama-endpoint'] ?? 'http://127.0.0.1:11434',
          model: values['ollama-model'] ?? 'nomic-embed-text',
        }
      : undefined;
  return {
    dbPath: utf8(8192).min(1).parse(values.db),
    project: identity.parse(values.project),
    agent: identity.parse(values.agent),
    workerPath: values.worker,
    chromePath: values.chrome,
    searxngUrl: values.searxng,
    ollama,
  };
}

export async function readInput(value?: string, signal?: AbortSignal): Promise<unknown> {
  signal?.throwIfAborted();
  if (value !== undefined) return JSON.parse(utf8(16 * MIB).parse(value));
  if (process.stdin.isTTY) throw new Error('Supply operation JSON through stdin or --input');
  const chunks: Buffer[] = [];
  let size = 0;
  const abort = () => process.stdin.destroy(new Error('CLI input was cancelled'));
  signal?.addEventListener('abort', abort, { once: true });
  try {
    for await (const chunk of process.stdin) {
      const bytes = Buffer.from(chunk);
      size += bytes.length;
      if (size > 16 * MIB) throw new Error('CLI input exceeds 16 MiB');
      chunks.push(bytes);
    }
  } finally {
    signal?.removeEventListener('abort', abort);
  }
  return JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(Buffer.concat(chunks, size)));
}

export const HELP = `n2-kraken <extract|fetch|render|search|research|learn|get|history|mcp>
  --db <mimir.db> --project <project> --agent <agent>
  [--worker <executable>] [--chrome <executable>] [--searxng <trusted-service-url>]
  [--ollama-endpoint <trusted-service-url>] [--ollama-model <model>]
Retrieval and research read operation JSON from stdin, or --input <json>.
learn/get require --id <research-request-id>. history accepts --limit and --offset.
Research stores provenance. learn explicitly adds unverified evidence to Mimir.
`;
