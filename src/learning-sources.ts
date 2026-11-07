// Source ownership and observation order are checked before invalidating shared reasoning evidence.
import { z } from 'zod';
import type { KrakenStore } from './store';
import type { CrawledPage } from './report';
import { digest, hash, parseJson, reference, webUrl } from './values';

export function currentSource(store: KrakenStore, url: string) {
  const row = store.connection.query<Record<string, unknown>>(
    'SELECT content_hash,experience_id,observed_at FROM kraken_sources WHERE project=? AND agent=? AND url=?',
    [...store.scope, url],
  )[0];
  if (!row) return undefined;
  return {
    hash: hash.parse(row.content_hash),
    id: reference.parse(row.experience_id),
    observedAt: z.int().min(0).parse(row.observed_at),
  };
}

export function ownedSource(
  store: KrakenStore,
  id: string,
  url: string,
  contentHash: string,
): boolean {
  const row = store.connection.query<Record<string, unknown>>(
    `SELECT project,agent,category,type,
      CASE WHEN length(CAST(context AS BLOB))<=32768 THEN context ELSE NULL END AS context,
      CASE WHEN length(CAST(outcome AS BLOB))<=100000 THEN outcome ELSE NULL END AS outcome
      FROM experiences WHERE id=?`,
    [id],
  )[0];
  if (!row) return false;
  const evidence = z
    .object({ producer: z.literal('n2-kraken'), source_url: webUrl, content_hash: hash })
    .parse(parseJson(row.context, 32768));
  if (
    row.project !== store.project ||
    row.agent !== store.agent ||
    row.category !== 'web_source' ||
    row.type !== 'pattern' ||
    evidence.source_url !== url ||
    evidence.content_hash !== contentHash ||
    typeof row.outcome !== 'string' ||
    digest(row.outcome) !== contentHash
  )
    throw new Error('Research source refers to a different owner or altered evidence');
  return true;
}

export function sourceIsValid(store: KrakenStore, id: string): boolean {
  return (
    store.connection.query('SELECT 1 FROM mimir_evidence_invalidations WHERE experience_id=?', [id])
      .length === 0
  );
}

export function retainSource(
  store: KrakenStore,
  page: CrawledPage,
): { id: string; created: boolean } {
  const previous = currentSource(store, page.url);
  const observedAt = Number(page.fetched_at) * 1000;
  if (previous && previous.hash !== page.content_hash && previous.observedAt >= observedAt)
    throw new Error(
      'Research contains an older or conflicting observation; run fresh research before learning',
    );
  if (previous && ownedSource(store, previous.id, page.url, previous.hash)) {
    if (previous.hash === page.content_hash && sourceIsValid(store, previous.id)) {
      store.connection.execute(
        'UPDATE kraken_sources SET observed_at=max(observed_at,?) WHERE project=? AND agent=? AND url=?',
        [observedAt, ...store.scope, page.url],
      );
      return { id: previous.id, created: false };
    }
    store.memory.reasoning.invalidateEvidence(previous.id);
  }
  const experience = store.memory.insertExperience({
    project: store.project,
    agent: store.agent,
    type: 'pattern',
    category: 'web_source',
    context: JSON.stringify({
      producer: 'n2-kraken',
      source_url: page.url,
      content_hash: page.content_hash,
      truncated: page.truncated,
      fetched_at: page.fetched_at,
      verified: false,
    }),
    action: `Retrieved public web content from ${page.url}`,
    outcome: page.content,
    sourceRef: `web:${encodeURIComponent(page.url)}?sha256=${page.content_hash}`,
  });
  store.connection.execute(
    `INSERT INTO kraken_sources VALUES(?,?,?,?,?,?) ON CONFLICT(project,agent,url)
    DO UPDATE SET content_hash=excluded.content_hash,experience_id=excluded.experience_id,observed_at=excluded.observed_at`,
    [...store.scope, page.url, page.content_hash, experience.id, observedAt],
  );
  return { id: experience.id, created: true };
}
