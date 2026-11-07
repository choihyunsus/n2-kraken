// Immutable request IDs replay completed research and reject conflicting inputs.
import { z } from 'zod';
import type { KrakenStore } from './store';
import { researchReport, type ResearchReport } from './report';
import { digest, encode, hash, identity, MIB, parseJson } from './values';

export interface StoredResearch {
  requestId: string;
  inputHash: string;
  workerHash: string;
  reportHash: string;
  createdAt: number;
  report: ResearchReport;
}
export const MAX_REPORT_BYTES = 4 * MIB;

export function readResearch(store: KrakenStore, requestId: string): StoredResearch | undefined {
  identity.parse(requestId);
  const row = store.connection.query<Record<string, unknown>>(
    `SELECT input_hash,worker_hash,report_hash,created_at,
      CASE WHEN length(CAST(report_json AS BLOB))<=? THEN report_json ELSE NULL END AS report_json
      FROM kraken_research WHERE project=? AND agent=? AND request_id=?`,
    [MAX_REPORT_BYTES, ...store.scope, requestId],
  )[0];
  if (!row) return undefined;
  const reportHash = hash.parse(row.report_hash);
  if (typeof row.report_json !== 'string' || digest(row.report_json) !== reportHash)
    throw new Error('Research receipt is oversized or its digest does not match');
  const report = researchReport.parse(parseJson(row.report_json, MAX_REPORT_BYTES));
  return {
    requestId,
    inputHash: hash.parse(row.input_hash),
    workerHash: hash.parse(row.worker_hash),
    reportHash,
    createdAt: z.int().min(0).parse(row.created_at),
    report,
  };
}

export function storeResearch(
  store: KrakenStore,
  requestId: string,
  inputHash: string,
  workerHash: string,
  report: ResearchReport,
  signal?: AbortSignal,
): StoredResearch {
  const encoded = encode(researchReport.parse(report), MAX_REPORT_BYTES),
    reportHash = digest(encoded);
  return store.connection.transaction(() => {
    signal?.throwIfAborted();
    const repeated = readResearch(store, requestId);
    if (repeated) return matchingInput(repeated, inputHash);
    store.connection.execute('INSERT INTO kraken_research VALUES(?,?,?,?,?,?,?,?)', [
      ...store.scope,
      identity.parse(requestId),
      hash.parse(inputHash),
      hash.parse(workerHash),
      reportHash,
      encoded,
      Date.now(),
    ]);
    return readResearch(store, requestId)!;
  });
}

export function matchingInput(result: StoredResearch, inputHash: string): StoredResearch {
  if (result.inputHash !== inputHash)
    throw new Error('Research request ID already belongs to different inputs');
  return result;
}

export function listResearch(store: KrakenStore, limit = 20, offset = 0) {
  z.int().min(1).max(100).parse(limit);
  z.int().min(0).max(100000).parse(offset);
  return store.connection
    .query<{ request_id: string; report_hash: string; created_at: number }>(
      `SELECT request_id,report_hash,created_at FROM kraken_research WHERE project=? AND agent=?
      ORDER BY created_at DESC,request_id LIMIT ? OFFSET ?`,
      [...store.scope, limit, offset],
    )
    .map((row) => ({
      requestId: identity.parse(row.request_id),
      reportHash: hash.parse(row.report_hash),
      createdAt: z.int().min(0).parse(row.created_at),
    }));
}
