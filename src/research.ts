// Research is persisted only after complete bounded validation, and matching retries reuse it.
import { researchInput, type ResearchInput } from './input';
import { researchReport, type ResearchReport } from './report';
import { matchingInput, readResearch, storeResearch, type StoredResearch } from './research-store';
import type { KrakenStore } from './store';
import type { NativeWorker } from './worker';
import { digest, encode } from './values';

export async function runResearch(
  store: KrakenStore,
  worker: NativeWorker,
  input: ResearchInput,
  signal: AbortSignal,
): Promise<StoredResearch> {
  const request = researchInput.parse(input),
    configuration = worker.configuration();
  const { requestId, ...arguments_ } = request;
  const command = {
    ...arguments_,
    config: { ...request.config, searxng_url: configuration.searxngUrl },
  };
  const inputHash = digest(
    encode(
      {
        command,
        worker: configuration.fingerprint,
        ollama: [
          configuration.environment.OLLAMA_ENDPOINT ?? null,
          configuration.environment.OLLAMA_EMBED_MODEL ?? null,
        ],
      },
      32768,
    ),
  );
  const repeated = readResearch(store, requestId);
  if (repeated) return matchingInput(repeated, inputHash);
  const raw = await worker.call(
    'research',
    command,
    request.config.total_timeout_ms + 10000,
    signal,
  );
  const report = researchReport.parse(raw);
  checkRequestedLimits(report, request);
  return storeResearch(store, requestId, inputHash, configuration.fingerprint, report, signal);
}

function checkRequestedLimits(
  report: ResearchReport,
  request: ReturnType<typeof researchInput.parse>,
): void {
  if (
    report.summary.topic !== request.topic ||
    (!request.semantic && report.summary.semantic_mode) ||
    report.summary.search_results > request.config.max_results ||
    report.pages.length > request.config.max_crawl_pages ||
    report.summary.claims_extracted > report.pages.length * 15 ||
    report.pages.some((page) => page.content_length > request.config.max_content_length) ||
    report.facts.some(
      (fact) => fact.status === 'Verified' && fact.confidence < request.config.min_confidence,
    )
  )
    throw new Error('Native research output exceeds its requested limits');
}
