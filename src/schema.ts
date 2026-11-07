// Module tables add scoped receipts while preserving the shared Soul/Mimir schema.
import type { SqlConnection } from './store';

export function initializeSchema(connection: SqlConnection): void {
  connection.transaction(() => {
    const version = connection.query<{ value: unknown }>(
      "SELECT value FROM meta WHERE key='kraken_schema_version'",
    )[0]?.value;
    const tables = connection.query<{ name: string }>(
      "SELECT name FROM sqlite_master WHERE type='table' AND name GLOB 'kraken_*'",
    );
    if (version !== undefined && version !== '1')
      throw new Error('Unsupported Kraken schema version');
    if (tables.length && version === undefined)
      throw new Error('Unversioned Kraken tables require an explicit migration');
    for (const table of tables) {
      const expected = COLUMNS[table.name];
      const columns = connection.query<{ name: string }>('SELECT name FROM pragma_table_info(?)', [
        table.name,
      ]);
      if (!expected || expected.join(',') !== columns.map((column) => column.name).join(','))
        throw new Error('Existing Kraken tables have an incompatible schema');
    }
    for (const sql of STATEMENTS) connection.execute(sql);
    connection.execute("INSERT OR IGNORE INTO meta(key,value) VALUES('kraken_schema_version','1')");
  });
}

const COLUMNS: Record<string, string[]> = {
  kraken_research: [
    'project',
    'agent',
    'request_id',
    'input_hash',
    'worker_hash',
    'report_hash',
    'report_json',
    'created_at',
  ],
  kraken_sources: ['project', 'agent', 'url', 'content_hash', 'experience_id', 'observed_at'],
  kraken_learning: [
    'project',
    'agent',
    'request_id',
    'policy_id',
    'report_hash',
    'accepted_json',
    'source_ids_json',
    'insight_ids_json',
    'learned_at',
  ],
  kraken_claims: ['project', 'agent', 'claim_hash', 'evidence_hash', 'insight_id'],
};
const STATEMENTS = [
  `CREATE TABLE IF NOT EXISTS kraken_research (
    project TEXT NOT NULL, agent TEXT NOT NULL, request_id TEXT NOT NULL,
    input_hash TEXT NOT NULL, worker_hash TEXT NOT NULL, report_hash TEXT NOT NULL,
    report_json TEXT NOT NULL CHECK(length(CAST(report_json AS BLOB))<=4194304), created_at INTEGER NOT NULL,
    PRIMARY KEY(project,agent,request_id))`,
  `CREATE INDEX IF NOT EXISTS idx_kraken_research_scope ON kraken_research(project,agent,created_at DESC,request_id)`,
  `CREATE TABLE IF NOT EXISTS kraken_sources (
    project TEXT NOT NULL, agent TEXT NOT NULL, url TEXT NOT NULL, content_hash TEXT NOT NULL,
    experience_id TEXT NOT NULL, observed_at INTEGER NOT NULL, PRIMARY KEY(project,agent,url))`,
  `CREATE TABLE IF NOT EXISTS kraken_learning (
    project TEXT NOT NULL, agent TEXT NOT NULL, request_id TEXT NOT NULL, policy_id TEXT NOT NULL,
    report_hash TEXT NOT NULL, accepted_json TEXT NOT NULL, source_ids_json TEXT NOT NULL,
    insight_ids_json TEXT NOT NULL, learned_at INTEGER NOT NULL,
    PRIMARY KEY(project,agent,request_id,policy_id),
    FOREIGN KEY(project,agent,request_id) REFERENCES kraken_research(project,agent,request_id))`,
  `CREATE TABLE IF NOT EXISTS kraken_claims (
    project TEXT NOT NULL, agent TEXT NOT NULL, claim_hash TEXT NOT NULL, evidence_hash TEXT NOT NULL,
    insight_id TEXT NOT NULL, PRIMARY KEY(project,agent,claim_hash,evidence_hash))`,
];
