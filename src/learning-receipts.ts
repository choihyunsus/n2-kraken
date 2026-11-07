// Reuse learning receipts only while their scoped sources, links and conclusions remain intact.
import { z } from 'zod';
import type { KrakenStore } from './store';
import type { StoredResearch } from './research-store';
import type { LearningReceipt } from './learning-types';
import { hash, parseJson, reference, webUrl } from './values';
import { currentSource, ownedSource, sourceIsValid } from './learning-sources';
import { currentInsight } from './learning-insights';

const indices = z
  .array(z.int().min(0).max(299))
  .max(300)
  .refine((values) => new Set(values).size === values.length);
const sources = z.array(z.strictObject({ url: webUrl, id: reference })).max(20);
const insights = z
  .array(z.strictObject({ index: z.int().min(0).max(299), id: reference }))
  .max(300);

export function readLearning(
  store: KrakenStore,
  requestId: string,
  policyId: string,
): LearningReceipt | undefined {
  const row = store.connection.query<Record<string, unknown>>(
    `SELECT report_hash,learned_at,
      CASE WHEN length(accepted_json)<=2048 THEN accepted_json ELSE NULL END AS accepted_json,
      CASE WHEN length(CAST(source_ids_json AS BLOB))<=262144 THEN source_ids_json ELSE NULL END AS source_ids_json,
      CASE WHEN length(insight_ids_json)<=32768 THEN insight_ids_json ELSE NULL END AS insight_ids_json
      FROM kraken_learning WHERE project=? AND agent=? AND request_id=? AND policy_id=?`,
    [...store.scope, requestId, policyId],
  )[0];
  if (!row) return undefined;
  const receipt = {
    requestId,
    policyId,
    reportHash: hash.parse(row.report_hash),
    accepted: indices.parse(parseJson(row.accepted_json, 2048)),
    sources: sources.parse(parseJson(row.source_ids_json, 262144)),
    insights: insights.parse(parseJson(row.insight_ids_json, 32768)),
    learnedAt: z.int().min(0).parse(row.learned_at),
  };
  if (
    new Set(receipt.sources.map((source) => source.url)).size !== receipt.sources.length ||
    new Set(receipt.sources.map((source) => source.id)).size !== receipt.sources.length ||
    new Set(receipt.insights.map((insight) => insight.id)).size !== receipt.insights.length ||
    receipt.insights.length !== receipt.accepted.length ||
    receipt.insights.some((insight, position) => insight.index !== receipt.accepted[position])
  )
    throw new Error('Research learning receipt has inconsistent references');
  return receipt;
}

export function currentLearning(
  store: KrakenStore,
  research: StoredResearch,
  policyId: string,
): LearningReceipt | undefined {
  const receipt = readLearning(store, research.requestId, policyId);
  if (!receipt) return undefined;
  if (receipt.reportHash !== research.reportHash)
    throw new Error('Learning receipt does not match its research');
  const expected = new Set(
    receipt.accepted.flatMap((index) => {
      const fact = research.report.facts[index];
      if (!fact || fact.status !== 'Verified')
        throw new Error('Learning receipt selects an ineligible fact');
      return fact.sources;
    }),
  );
  if (
    receipt.sources.length !== expected.size ||
    receipt.sources.some((source) => !expected.has(source.url))
  )
    throw new Error('Learning receipt has inconsistent source coverage');
  for (const source of receipt.sources) {
    const page = research.report.pages.find((page) => page.url === source.url)!;
    if (
      !ownedSource(store, source.id, source.url, page.content_hash) ||
      !sourceIsValid(store, source.id)
    )
      return undefined;
    const current = currentSource(store, source.url);
    if (current?.id !== source.id || current.hash !== page.content_hash) return undefined;
  }
  return receipt.insights.every((insight) => {
    const fact = research.report.facts[insight.index]!;
    const ids = fact.sources.map((url) => receipt.sources.find((source) => source.url === url)!.id);
    return currentInsight(store, insight.id, fact.claim, ids);
  })
    ? receipt
    : undefined;
}
