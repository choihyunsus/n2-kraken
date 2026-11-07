// One Mimir connection owns transactions across research receipts and common learning tables.
import fs from 'node:fs';
import path from 'node:path';
import { MimirDatabase } from 'n2-mimir/storage';
import { identity, utf8 } from './values';
import { initializeSchema } from './schema';

export type SqlConnection = MimirDatabase['reasoning']['connection'];
export interface StoreOptions {
  dbPath: string;
  project: string;
  agent: string;
}

export class KrakenStore {
  readonly memory: MimirDatabase;
  readonly connection: SqlConnection;
  readonly project: string;
  readonly agent: string;
  readonly dbPath: string;

  constructor(options: StoreOptions) {
    this.project = identity.parse(options.project);
    this.agent = identity.parse(options.agent);
    const filename = utf8(8192)
      .min(1)
      .refine((value) => !value.includes('\0'))
      .parse(options.dbPath);
    this.dbPath = filename === ':memory:' ? filename : path.resolve(filename);
    if (this.dbPath !== ':memory:') fs.mkdirSync(path.dirname(this.dbPath), { recursive: true });
    this.memory = new MimirDatabase(this.dbPath);
    this.connection = this.memory.reasoning.connection;
    try {
      initializeSchema(this.connection);
    } catch (error) {
      this.memory.close();
      throw error;
    }
  }

  get scope(): [string, string] {
    return [this.project, this.agent];
  }
  close(): void {
    this.memory.close();
  }
}
