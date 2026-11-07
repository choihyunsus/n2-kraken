// Fingerprint an explicit regular executable and pass only documented service configuration.
import fs from 'node:fs';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { identity, MIB, utf8, webUrl } from './values';

export interface WorkerOptions {
  workerPath?: string;
  chromePath?: string;
  searxngUrl?: string;
  ollama?: { endpoint: string; model: string };
}
export interface ResolvedWorker {
  filename: string;
  fingerprint: string;
  environment: NodeJS.ProcessEnv;
  searxngUrl: string;
}

export function resolveWorker(options: WorkerOptions): ResolvedWorker {
  const filename = fs.realpathSync.native(
    options.workerPath ??
      path.resolve(
        __dirname,
        '../bin',
        `n2-kraken-worker-${process.platform}-${process.arch}${process.platform === 'win32' ? '.exe' : ''}`,
      ),
  );
  const environment: NodeJS.ProcessEnv = {};
  for (const key of [
    'SystemRoot',
    'WINDIR',
    'PATH',
    'PROGRAMFILES',
    'PROGRAMFILES(X86)',
    'LOCALAPPDATA',
    'LANG',
  ])
    if (process.env[key] !== undefined) environment[key] = process.env[key];
  if (options.chromePath !== undefined) {
    const chrome = fs.realpathSync.native(utf8(8192).min(1).parse(options.chromePath));
    if (!fs.statSync(chrome).isFile()) throw new Error('Chrome executable must be a regular file');
    environment.CHROME_PATH = chrome;
  }
  if (options.ollama !== undefined) {
    environment.OLLAMA_ENDPOINT = serviceUrl(options.ollama.endpoint);
    environment.OLLAMA_EMBED_MODEL = identity.parse(options.ollama.model);
  }
  const searxngUrl = options.searxngUrl === undefined ? '' : serviceUrl(options.searxngUrl);
  if (searxngUrl) environment.SEARXNG_URL = searxngUrl;
  return { filename, fingerprint: executableHash(filename), environment, searxngUrl };
}

export function executableHash(filename: string): string {
  const descriptor = fs.openSync(filename, fs.constants.O_RDONLY);
  try {
    const before = fs.fstatSync(descriptor);
    if (!before.isFile() || before.size === 0 || before.size > 256 * MIB)
      throw new Error('Worker executable must be a regular file within 256 MiB');
    const hash = createHash('sha256'),
      buffer = Buffer.alloc(64 * 1024);
    let count: number,
      total = 0;
    while ((count = fs.readSync(descriptor, buffer, 0, buffer.length, null)) > 0) {
      total += count;
      if (total > 256 * MIB) throw new Error('Worker executable exceeds its byte limit');
      hash.update(buffer.subarray(0, count));
    }
    const after = fs.fstatSync(descriptor),
      current = fs.statSync(filename);
    if (
      total !== before.size ||
      after.size !== before.size ||
      after.mtimeMs !== before.mtimeMs ||
      after.ctimeMs !== before.ctimeMs ||
      current.ino !== before.ino ||
      current.dev !== before.dev
    )
      throw new Error('Worker executable changed while being fingerprinted');
    return hash.digest('hex');
  } finally {
    fs.closeSync(descriptor);
  }
}

function serviceUrl(value: string): string {
  const url = new URL(webUrl.parse(value));
  if (url.search || url.hash) throw new Error('Service URL cannot contain a query or fragment');
  return url.toString();
}
