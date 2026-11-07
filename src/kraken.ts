// Public operations use one scoped store and cancel owned workers before closing that connection.
import { KrakenStore, type StoreOptions } from './store';
import { NativeWorker } from './worker';
import type { WorkerOptions } from './worker-path';
import { extractInput, fetchInput, renderInput, searchInput } from './input';
import type { ExtractInput, FetchInput, RenderInput, SearchInput, ResearchInput } from './input';
import { checkedMarkdown, checkedRender, searchResults } from './report';
import { runResearch } from './research';
import { readResearch, listResearch } from './research-store';
import { learnResearch } from './learning';
import type { LearningPolicy } from './learning-types';
import { readLearning } from './learning-receipts';
import { identity } from './values';

export interface KrakenOptions extends StoreOptions, WorkerOptions {}

export class Kraken {
  private readonly store: KrakenStore;
  private readonly worker: NativeWorker;
  private readonly shutdown = new AbortController();
  private readonly pending = new Set<Promise<unknown>>();
  private closing?: Promise<void>;

  constructor(options: KrakenOptions) {
    this.worker = new NativeWorker({ ...options, ollama: options.ollama && { ...options.ollama } });
    this.store = new KrakenStore(options);
  }

  extract(input: ExtractInput, signal?: AbortSignal) {
    const request = extractInput.parse(input);
    return this.perform(async (signal) => {
      const document = checkedMarkdown(
        await this.worker.call('extract', request, 30000, signal),
        request.config.max_length,
      );
      let expected = '';
      if (request.source_url) {
        const url = new URL(request.source_url);
        url.hash = '';
        expected = url.toString();
      }
      if (document.source_url !== expected)
        throw new Error('Extracted source identity does not match the supplied URL');
      return document;
    }, signal);
  }

  fetch(input: FetchInput, signal?: AbortSignal) {
    const request = fetchInput.parse(input);
    return this.perform(
      async (signal) =>
        checkedMarkdown(
          await this.worker.call('fetch', request, 70000, signal),
          request.config.max_length,
        ),
      signal,
    );
  }

  render(input: RenderInput, signal?: AbortSignal) {
    const request = renderInput.parse(input);
    return this.perform(
      async (signal) =>
        checkedRender(
          await this.worker.call('render', request, 50000, signal),
          request.max_length,
          request.format,
        ),
      signal,
    );
  }

  search(input: SearchInput, signal?: AbortSignal) {
    const request = searchInput.parse(input);
    return this.perform(async (signal) => {
      const config = { ...request.config, searxng_url: this.worker.configuration().searxngUrl };
      const results = searchResults.parse(
        await this.worker.call(
          'search',
          { ...request, config },
          config.total_timeout_ms + 10000,
          signal,
        ),
      );
      if (results.length > config.max_results)
        throw new Error('Native search exceeds its requested limit');
      return results;
    }, signal);
  }

  research(input: ResearchInput, signal?: AbortSignal) {
    return this.perform((signal) => runResearch(this.store, this.worker, input, signal), signal);
  }

  learn(requestId: string, options: { policy?: LearningPolicy; signal?: AbortSignal } = {}) {
    return this.perform(
      (signal) => learnResearch(this.store, requestId, options.policy, signal),
      options.signal,
    );
  }

  getResearch(requestId: string) {
    this.assertOpen();
    return readResearch(this.store, requestId);
  }
  listResearch(limit = 20, offset = 0) {
    this.assertOpen();
    return listResearch(this.store, limit, offset);
  }

  getLearning(requestId: string, policyId = 'source-agreement-v1') {
    this.assertOpen();
    identity.parse(requestId);
    identity.parse(policyId);
    const receipt = readLearning(this.store, requestId, policyId);
    if (receipt && readResearch(this.store, requestId)?.reportHash !== receipt.reportHash)
      throw new Error('Learning receipt does not match its research');
    return receipt;
  }

  close(): Promise<void> {
    if (this.closing) return this.closing;
    this.shutdown.abort();
    return (this.closing = Promise.allSettled([...this.pending]).then(() => this.store.close()));
  }

  private perform<T>(
    operation: (signal: AbortSignal) => Promise<T>,
    signal?: AbortSignal,
  ): Promise<T> {
    this.assertOpen();
    const combined = signal
      ? AbortSignal.any([signal, this.shutdown.signal])
      : this.shutdown.signal;
    combined.throwIfAborted();
    const pending = Promise.resolve().then(() => {
      combined.throwIfAborted();
      return operation(combined);
    });
    this.pending.add(pending);
    return pending.finally(() => this.pending.delete(pending));
  }

  private assertOpen(): void {
    if (this.shutdown.signal.aborted) throw new Error('Kraken is closed');
  }
}
