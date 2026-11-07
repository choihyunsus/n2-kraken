// Versioned one-shot RPC isolates web parsing and owns a temporary worker directory.
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { spawn } from 'node:child_process';
import { randomUUID } from 'node:crypto';
import { z } from 'zod';
import { collectWorker } from './worker-process';
import {
  executableHash,
  resolveWorker,
  type WorkerOptions,
  type ResolvedWorker,
} from './worker-path';
import { encode, identity, MIB, parseJson, utf8 } from './values';

const success = z.strictObject({
  protocol: z.literal(1),
  id: identity,
  success: z.literal(true),
  data: z.unknown(),
});
const failed = z.strictObject({
  protocol: z.literal(1),
  id: identity.nullable(),
  success: z.literal(false),
  error: z.strictObject({ code: utf8(128), message: utf8(16384) }),
});
let active = 0;

export class NativeWorker {
  private resolved?: ResolvedWorker;
  constructor(private readonly options: WorkerOptions) {}

  configuration(): ResolvedWorker {
    return (this.resolved ??= resolveWorker(this.options));
  }

  async call(
    operation: string,
    arguments_: unknown,
    timeoutMs: number,
    signal?: AbortSignal,
  ): Promise<unknown> {
    signal?.throwIfAborted();
    if (active >= 2)
      throw new Error('Two Kraken workers are already running; retry after they finish');
    const worker = this.configuration();
    if (executableHash(worker.filename) !== worker.fingerprint)
      throw new Error('Worker executable changed; reopen Kraken before another operation');
    const id = randomUUID(),
      input = encode({ protocol: 1, id, command: { operation, arguments: arguments_ } }, 16 * MIB);
    active++;
    let directory: string | undefined;
    try {
      directory = await fs.mkdtemp(path.join(os.tmpdir(), 'n2-kraken-worker-'));
      signal?.throwIfAborted();
      const child = spawn(worker.filename, [], {
        shell: false,
        windowsHide: true,
        detached: process.platform !== 'win32',
        stdio: ['pipe', 'pipe', 'pipe'],
        cwd: directory,
        env: { ...worker.environment, TEMP: directory, TMP: directory, TMPDIR: directory },
      });
      const result = await collectWorker(child, input, timeoutMs, signal);
      const response = z.union([success, failed]).parse(parseJson(result.output, 32 * MIB));
      if (response.id !== id)
        throw new Error('Native response identifier does not match its request');
      if (!response.success)
        throw new Error(`Kraken ${response.error.code}: ${response.error.message}`);
      if (result.code !== 0) throw new Error('Native worker failed after producing a result');
      signal?.throwIfAborted();
      return response.data;
    } finally {
      active--;
      if (directory) await removeOwnedDirectory(directory);
    }
  }
}

async function removeOwnedDirectory(directory: string): Promise<void> {
  const root = await fs.realpath(os.tmpdir()),
    resolved = await fs.realpath(directory);
  if (path.dirname(resolved) !== root || !path.basename(resolved).startsWith('n2-kraken-worker-'))
    throw new Error('Worker temporary directory changed; cleanup refused');
  await fs.rm(resolved, { recursive: true, force: true, maxRetries: 3, retryDelay: 100 });
}
