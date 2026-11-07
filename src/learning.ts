// Screening precedes one transaction that publishes evidence, unverified insights and its receipt.
import type { KrakenStore } from './store';
import type { StoredResearch } from './research-store';
import { readResearch } from './research-store';
import type { LearningPolicy, LearningReceipt, LearnResult } from './learning-types';
import { policyIdentity, selectFacts } from './learning-policy';
import { currentLearning, readLearning } from './learning-receipts';
import { retainSource } from './learning-sources';
import { retainInsight } from './learning-insights';

export async function learnResearch(
  store: KrakenStore,
  requestId: string,
  policy?: LearningPolicy,
  signal?: AbortSignal,
): Promise<LearnResult> {
  signal?.throwIfAborted();
  const policyId = policyIdentity(policy),
    research = readResearch(store, requestId);
  if (!research) throw new Error('Research request was not found in this project and agent scope');
  const repeated = store.connection.transaction(() => currentLearning(store, research, policyId));
  if (repeated) return repeatedResult(repeated);
  const previous = readLearning(store, requestId, policyId);
  const accepted = previous?.accepted ?? (await selectFacts(research, policy, signal));
  return store.connection.transaction(() => publish(store, research, policyId, accepted, signal));
}

function publish(
  store: KrakenStore,
  research: StoredResearch,
  policyId: string,
  accepted: number[],
  signal?: AbortSignal,
): LearnResult {
  signal?.throwIfAborted();
  const current = readResearch(store, research.requestId);
  if (current?.reportHash !== research.reportHash)
    throw new Error('Research changed during policy review');
  const repeated = currentLearning(store, research, policyId);
  if (repeated) return repeatedResult(repeated);
  const deadline = Date.now() + 15000;
  const evidence = publishEvidence(store, research, accepted, deadline);
  const receipt = {
    requestId: research.requestId,
    policyId,
    reportHash: research.reportHash,
    accepted,
    sources: evidence.sources,
    insights: evidence.insights,
    learnedAt: Date.now(),
  };
  signal?.throwIfAborted();
  checkDeadline(deadline);
  recordLearning(store, receipt);
  return {
    ...receipt,
    status: 'learned',
    verified: false,
    newSources: evidence.newSources,
    newInsights: evidence.newInsights,
  };
}

function publishEvidence(
  store: KrakenStore,
  research: StoredResearch,
  accepted: number[],
  deadline: number,
) {
  let newSources = 0,
    newInsights = 0;
  const urls = new Set(accepted.flatMap((index) => research.report.facts[index]!.sources));
  const sources = research.report.pages
    .filter((page) => urls.has(page.url))
    .map((page) => {
      checkDeadline(deadline);
      const source = retainSource(store, page);
      newSources += Number(source.created);
      return { url: page.url, id: source.id, created: source.created };
    });
  const insights = accepted.map((index) => {
    checkDeadline(deadline);
    const fact = research.report.facts[index]!;
    const ids = fact.sources.map((url) => sources.find((source) => source.url === url)!.id);
    const insight = retainInsight(store, fact, ids);
    newInsights += Number(insight.created);
    return { index, id: insight.id, created: insight.created };
  });
  return { sources, insights, newSources, newInsights };
}

function recordLearning(store: KrakenStore, receipt: LearningReceipt): void {
  store.connection.execute(
    `INSERT INTO kraken_learning VALUES(?,?,?,?,?,?,?,?,?)
    ON CONFLICT(project,agent,request_id,policy_id) DO UPDATE SET report_hash=excluded.report_hash,
    accepted_json=excluded.accepted_json,source_ids_json=excluded.source_ids_json,
    insight_ids_json=excluded.insight_ids_json,learned_at=excluded.learned_at`,
    [
      ...store.scope,
      receipt.requestId,
      receipt.policyId,
      receipt.reportHash,
      JSON.stringify(receipt.accepted),
      JSON.stringify(receipt.sources),
      JSON.stringify(receipt.insights),
      receipt.learnedAt,
    ],
  );
}

function repeatedResult(receipt: LearningReceipt): LearnResult {
  return { ...receipt, status: 'already_learned', verified: false, newSources: 0, newInsights: 0 };
}

function checkDeadline(deadline: number): void {
  if (Date.now() > deadline)
    throw new Error('Learning exceeded 15 seconds; its transaction was rolled back');
}
