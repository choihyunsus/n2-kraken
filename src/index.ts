#!/usr/bin/env node
// CLI and MCP entry points share the public scoped engine and owned shutdown lifecycle.
import { Kraken } from './kraken';
import { argumentsForCli, engineOptions, readInput, HELP, type CliArguments } from './cli-options';
import type { ExtractInput, FetchInput, RenderInput, SearchInput, ResearchInput } from './input';
import { identity } from './values';

async function main(): Promise<void> {
  const input = argumentsForCli(process.argv.slice(2)),
    command = input.positionals[0];
  if (input.values.version) {
    process.stdout.write('1.0.0\n');
    return;
  }
  if (input.values.help || !command) {
    process.stdout.write(HELP);
    return;
  }
  if (
    input.positionals.length !== 1 ||
    ![
      'extract',
      'fetch',
      'render',
      'search',
      'research',
      'learn',
      'get',
      'history',
      'mcp',
    ].includes(command)
  )
    throw new Error('Unknown command; use --help');
  const engine = new Kraken(engineOptions(input.values));
  if (command === 'mcp') {
    await serve(engine);
    return;
  }
  const controller = new AbortController(),
    stop = () => controller.abort();
  process.once('SIGINT', stop);
  process.once('SIGTERM', stop);
  try {
    const result = await execute(engine, command, input.values, controller.signal);
    process.stdout.write(JSON.stringify(result) + '\n');
  } finally {
    await engine.close();
    process.removeListener('SIGINT', stop);
    process.removeListener('SIGTERM', stop);
  }
}

async function execute(
  engine: Kraken,
  command: string,
  values: CliArguments['values'],
  signal: AbortSignal,
) {
  if (command === 'learn') return engine.learn(identity.parse(values.id), { signal });
  if (command === 'get') {
    const result = engine.getResearch(identity.parse(values.id));
    if (!result) throw new Error('Research was not found in this project and agent scope');
    return result;
  }
  if (command === 'history')
    return engine.listResearch(Number(values.limit ?? 20), Number(values.offset ?? 0));
  const input = await readInput(values.input, signal);
  signal.throwIfAborted();
  switch (command) {
    case 'extract':
      return engine.extract(input as ExtractInput, signal);
    case 'fetch':
      return engine.fetch(input as FetchInput, signal);
    case 'render':
      return engine.render(input as RenderInput, signal);
    case 'search':
      return engine.search(input as SearchInput, signal);
    case 'research':
      return engine.research(input as ResearchInput, signal);
    default:
      throw new Error('Unsupported operation');
  }
}

async function serve(engine: Kraken): Promise<void> {
  const { createMcpServer } = await import('./mcp.js');
  const { StdioServerTransport } = await import('@modelcontextprotocol/sdk/server/stdio.js');
  const server = createMcpServer(engine);
  const stop = () => {
    void server.close().finally(() => engine.close());
  };
  server.server.onclose = () => {
    process.removeListener('SIGINT', stop);
    process.removeListener('SIGTERM', stop);
    void engine.close();
  };
  process.once('SIGINT', stop);
  process.once('SIGTERM', stop);
  try {
    await server.connect(new StdioServerTransport());
  } catch (error) {
    await engine.close();
    throw error;
  }
}

main().catch((error: unknown) => {
  const message = error instanceof Error ? error.message : String(error);
  process.stderr.write(
    JSON.stringify({ error: Array.from(message).slice(0, 2000).join('') }) + '\n',
  );
  process.exitCode = 1;
});
