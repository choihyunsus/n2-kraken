// Equivalent claims over the same owned evidence reuse a scoped insight across research requests.
import type { KrakenStore } from './store';
import type { SourceFact } from './report';
import { digest, reference } from './values';

export function currentInsight(
  store: KrakenStore,
  id: string,
  claim: string,
  sourceIds: string[],
): boolean {
  const row = store.connection.query<Record<string, unknown>>(
    `SELECT agent,category,CASE WHEN length(CAST(description AS BLOB))<=8192 THEN description ELSE NULL END AS description
      FROM insights WHERE id=?`,
    [id],
  )[0];
  if (!row) return false;
  if (
    row.agent !== store.agent ||
    row.category !== 'web_research' ||
    row.description !== `Unverified source agreement: ${claim}`
  )
    throw new Error('Learning receipt refers to a different or altered insight');
  const expected = new Set(sourceIds);
  const linked = store.connection.query<{ experience_id: string }>(
    'SELECT experience_id FROM experience_insight_links WHERE insight_id=? LIMIT 21',
    [id],
  );
  if (linked.length !== expected.size || linked.some((row) => !expected.has(row.experience_id)))
    throw new Error('Research insight has inconsistent evidence links');
  return !!store.memory.reasoning.latestConclusion(id);
}

export function retainInsight(
  store: KrakenStore,
  fact: SourceFact,
  sourceIds: string[],
): { id: string; created: boolean } {
  const claimHash = digest(fact.claim),
    evidenceHash = digest(JSON.stringify([...sourceIds].sort()));
  const previous = store.connection.query<{ insight_id: unknown }>(
    'SELECT insight_id FROM kraken_claims WHERE project=? AND agent=? AND claim_hash=? AND evidence_hash=?',
    [...store.scope, claimHash, evidenceHash],
  )[0];
  if (previous) {
    const id = reference.parse(previous.insight_id);
    if (currentInsight(store, id, fact.claim, sourceIds)) return { id, created: false };
  }
  const insight = store.memory.insertInsight({
    project: store.project,
    agent: store.agent,
    scope: 'project',
    category: 'web_research',
    sourceIds,
    description: `Unverified source agreement: ${fact.claim}`,
    compressed: `Heuristic agreement ${fact.confidence.toFixed(3)} across ${fact.sources.length} URLs. Independent factual verification is required.`,
  });
  const conclusion = store.memory.reasoning.buildInsightChain(insight.id);
  if (!conclusion || conclusion.isVerified)
    throw new Error('Web source agreement must remain an unverified conclusion');
  store.connection.execute(
    `INSERT INTO kraken_claims VALUES(?,?,?,?,?)
    ON CONFLICT(project,agent,claim_hash,evidence_hash) DO UPDATE SET insight_id=excluded.insight_id`,
    [...store.scope, claimHash, evidenceHash, insight.id],
  );
  return { id: insight.id, created: true };
}
