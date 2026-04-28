/**
 * Datomic-compatible TypeScript/JavaScript client for pg_mentat.
 *
 * Implements the Datomic Client API protocol over WebSocket connections
 * using Transit+JSON encoding. This is a drop-in replacement for the
 * Datomic JS client.
 *
 * @example
 * ```typescript
 * import { client, connect, db, q, transact } from '@pg-mentat/client';
 *
 * const c = client({ endpoint: 'ws://localhost:8080/ws' });
 * const conn = await connect(c, { dbName: 'my-db' });
 * const database = await db(conn);
 * const results = await q('[:find ?e ?name :where [?e :person/name ?name]]', database);
 * await transact(conn, { txData: '[{:person/name "Alice"}]' });
 * ```
 */

import WebSocket from 'ws';
import { v4 as uuidv4 } from 'uuid';

// ---------------------------------------------------------------------------
// Transit+JSON encoding / decoding
// ---------------------------------------------------------------------------

/** Represents a Clojure/EDN keyword like :db/name. */
export class Keyword {
  readonly namespace: string | null;
  readonly name: string;

  constructor(name: string, namespace?: string) {
    if (namespace !== undefined) {
      this.namespace = namespace;
      this.name = name;
    } else if (name.includes('/') && !name.startsWith('/')) {
      const parts = name.split('/', 2);
      this.namespace = parts[0];
      this.name = parts[1];
    } else {
      this.namespace = null;
      this.name = name;
    }
  }

  toString(): string {
    return this.namespace ? `${this.namespace}/${this.name}` : this.name;
  }

  equals(other: Keyword): boolean {
    return this.namespace === other.namespace && this.name === other.name;
  }
}

/** Represents a Clojure/EDN symbol. */
export class Symbol {
  readonly name: string;
  constructor(name: string) {
    this.name = name;
  }
  toString(): string {
    return this.name;
  }
}

/** Keyword factory helper. */
export function kw(name: string): Keyword {
  return new Keyword(name);
}

type TransitValue = null | boolean | number | string | TransitValue[] | TransitMap;
interface TransitMap {
  [key: string]: TransitValue;
}

function transitEncodeValue(v: any): any {
  if (v === null || v === undefined) return null;
  if (typeof v === 'boolean') return v;
  if (v instanceof Keyword) return `~:${v.toString()}`;
  if (v instanceof Symbol) return `~$${v.name}`;
  if (typeof v === 'number') {
    if (Number.isInteger(v) && (v > 2_147_483_647 || v < -2_147_483_648)) {
      return `~i${v}`;
    }
    return v;
  }
  if (typeof v === 'string') {
    if (v.startsWith('~') || v.startsWith('^')) return `~${v}`;
    return v;
  }
  if (v instanceof Date) return `~m${v.getTime()}`;
  if (Array.isArray(v)) return v.map(transitEncodeValue);
  if (v instanceof Set) {
    return ['~#set', Array.from(v).map(transitEncodeValue)];
  }
  if (v instanceof Map) {
    const result: any[] = ['^ '];
    for (const [key, val] of v.entries()) {
      result.push(transitEncodeValue(key));
      result.push(transitEncodeValue(val));
    }
    return result;
  }
  if (typeof v === 'object') {
    const result: any[] = ['^ '];
    for (const [key, val] of Object.entries(v)) {
      result.push(transitEncodeValue(key));
      result.push(transitEncodeValue(val));
    }
    return result;
  }
  return String(v);
}

function transitEncode(m: any): string {
  return JSON.stringify(transitEncodeValue(m));
}

function transitDecodeTagged(s: string): any {
  if (s.startsWith('~:')) {
    return new Keyword(s.slice(2));
  }
  if (s.startsWith('~$')) return new Symbol(s.slice(2));
  if (s.startsWith('~i')) return parseInt(s.slice(2), 10);
  if (s.startsWith('~u')) return s.slice(2); // UUID as string
  if (s.startsWith('~m')) return new Date(parseInt(s.slice(2), 10));
  if (s === '~zNaN') return NaN;
  if (s === '~zINF') return Infinity;
  if (s === '~z-INF') return -Infinity;
  if (s.startsWith('~~')) return s.slice(1); // escaped tilde
  if (s.startsWith('~^')) return '^' + s.slice(2); // escaped caret
  return s;
}

function transitDecode(v: any): any {
  if (v === null || v === undefined) return null;
  if (typeof v === 'boolean' || typeof v === 'number') return v;
  if (typeof v === 'string') return transitDecodeTagged(v);
  if (Array.isArray(v)) {
    if (v.length > 0 && v[0] === '^ ') {
      // cmap: ["^ ", k1, v1, k2, v2, ...]
      const result: Map<any, any> = new Map();
      for (let i = 1; i + 1 < v.length; i += 2) {
        result.set(transitDecode(v[i]), transitDecode(v[i + 1]));
      }
      return result;
    }
    if (v.length === 2 && typeof v[0] === 'string') {
      if (v[0] === '~#list') return (v[1] as any[]).map(transitDecode);
      if (v[0] === '~#set') return new Set((v[1] as any[]).map(transitDecode));
    }
    return v.map(transitDecode);
  }
  if (typeof v === 'object') {
    const result: Map<any, any> = new Map();
    for (const [key, val] of Object.entries(v)) {
      result.set(transitDecode(key), transitDecode(val));
    }
    return result;
  }
  return v;
}

function parseTransitJson(s: string): any {
  return transitDecode(JSON.parse(s));
}

/** Helper to get a value from a decoded Transit map by keyword name. */
function mapGet(m: Map<any, any>, keyName: string): any {
  for (const [k, v] of m.entries()) {
    if (k instanceof Keyword && k.toString() === keyName) return v;
    if (typeof k === 'string' && k === keyName) return v;
  }
  return undefined;
}

// ---------------------------------------------------------------------------
// WebSocket connection
// ---------------------------------------------------------------------------

interface PendingRequest {
  resolve: (value: any) => void;
  reject: (error: Error) => void;
  timer: ReturnType<typeof setTimeout>;
}

class WsConnection {
  private ws: WebSocket | null = null;
  private pending: Map<string, PendingRequest> = new Map();
  private sessionId: string | null = null;
  private closed = false;
  private welcomeResolve: ((value: any) => void) | null = null;

  async connect(endpoint: string, apiKey?: string): Promise<void> {
    return new Promise((resolve, reject) => {
      const headers: Record<string, string> = {};
      if (apiKey) {
        headers['Authorization'] = `Bearer ${apiKey}`;
      }

      this.ws = new WebSocket(endpoint, { headers });
      let welcomed = false;

      this.ws.on('open', () => {
        // Wait for welcome message
      });

      this.ws.on('message', (data: WebSocket.Data) => {
        const msg = data.toString();
        const parsed = parseTransitJson(msg);

        if (!welcomed) {
          // First message should be the welcome
          welcomed = true;
          if (parsed instanceof Map) {
            this.sessionId = mapGet(parsed, 'session-id') ?? null;
          }
          resolve();
          return;
        }

        // Route by request-id
        if (parsed instanceof Map) {
          const rid = mapGet(parsed, 'request-id');
          if (rid && this.pending.has(rid)) {
            const req = this.pending.get(rid)!;
            clearTimeout(req.timer);
            this.pending.delete(rid);
            req.resolve(parsed);
            return;
          }
        }
      });

      this.ws.on('error', (err: Error) => {
        if (!welcomed) {
          reject(err);
          return;
        }
        // Reject all pending requests
        for (const [id, req] of this.pending) {
          clearTimeout(req.timer);
          req.reject(err);
        }
        this.pending.clear();
      });

      this.ws.on('close', () => {
        this.closed = true;
        if (!welcomed) {
          reject(new Error('WebSocket closed before welcome'));
          return;
        }
        for (const [id, req] of this.pending) {
          clearTimeout(req.timer);
          req.reject(new Error('Connection closed'));
        }
        this.pending.clear();
      });

      // Timeout for connection
      setTimeout(() => {
        if (!welcomed) {
          reject(new Error('Connection timeout'));
          this.ws?.close();
        }
      }, 10000);
    });
  }

  async sendRequest(request: any, timeoutMs: number = 30000): Promise<any> {
    if (this.closed || !this.ws) {
      throw new Error('WebSocket connection is closed');
    }

    const requestId = uuidv4();
    // Inject request-id into the Transit map
    if (request instanceof Map) {
      request.set(new Keyword('request-id'), requestId);
    } else if (typeof request === 'object') {
      request['~:request-id'] = requestId;
    }

    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(requestId);
        reject(new Error(`Request timed out after ${timeoutMs}ms`));
      }, timeoutMs);

      this.pending.set(requestId, { resolve, reject, timer });

      const msg = transitEncode(request);
      this.ws!.send(msg);
    });
  }

  close(): void {
    if (!this.closed && this.ws) {
      this.closed = true;
      this.ws.close(1000, 'client disconnect');
    }
  }

  get isConnected(): boolean {
    return !this.closed && this.ws !== null;
  }
}

// ---------------------------------------------------------------------------
// Datomic Client API types
// ---------------------------------------------------------------------------

export interface ClientConfig {
  /** WebSocket endpoint URL (e.g., "ws://localhost:8080/ws"). */
  endpoint: string;
  /** Optional API key for authentication. */
  apiKey?: string;
}

export interface PgMentatClient {
  readonly config: ClientConfig;
}

export interface PgMentatConnection {
  readonly client: PgMentatClient;
  readonly dbName: string;
  readonly connectionId: string;
  /** @internal */ readonly _ws: WsConnection;
}

export interface PgMentatDb {
  readonly connection: PgMentatConnection;
  readonly dbName: string;
  readonly databaseId: string;
  readonly t: number;
  readonly nextT: number;
  readonly asOfT?: number;
  readonly sinceT?: number;
  readonly isHistory?: boolean;
}

export class PgMentatError extends Error {
  readonly category: string;
  readonly response: any;

  constructor(message: string, category: string, response?: any) {
    super(message);
    this.name = 'PgMentatError';
    this.category = category;
    this.response = response;
  }
}

// ---------------------------------------------------------------------------
// Internal request builder
// ---------------------------------------------------------------------------

function buildRequest(op: string, args: Record<string, any>): any {
  const argsMap: any[] = ['^ '];
  for (const [key, val] of Object.entries(args)) {
    argsMap.push(`~:${key}`);
    argsMap.push(transitEncodeValue(val));
  }
  return ['^ ', '~:op', `~:${op}`, '~:args', argsMap];
}

async function sendAndExtract(ws: WsConnection, op: string,
                              args: Record<string, any>): Promise<any> {
  const request = buildRequest(op, args);
  const response = await ws.sendRequest(request);

  if (response instanceof Map) {
    const error = mapGet(response, 'error');
    if (error) {
      let message = 'Server error';
      let category = 'fault';
      if (error instanceof Map) {
        message = mapGet(error, 'cognitect.anomalies/message') ?? message;
        const cat = mapGet(error, 'cognitect.anomalies/category');
        if (cat) category = cat instanceof Keyword ? cat.toString() : String(cat);
      }
      throw new PgMentatError(message, category, response);
    }
    return mapGet(response, 'result');
  }
  return response;
}

// ---------------------------------------------------------------------------
// Datomic Client API functions
// ---------------------------------------------------------------------------

/**
 * Create a pg_mentat client.
 * Drop-in replacement for datomic client creation.
 */
export function client(config: ClientConfig): PgMentatClient {
  if (!config.endpoint) {
    throw new Error('Missing required endpoint in client config');
  }
  return { config };
}

/**
 * Connect to a database.
 * Drop-in replacement for datomic.client.api/connect.
 */
export async function connect(
  c: PgMentatClient,
  opts: { dbName: string }
): Promise<PgMentatConnection> {
  if (!opts.dbName) {
    throw new Error('Missing required dbName');
  }

  const ws = new WsConnection();
  await ws.connect(c.config.endpoint, c.config.apiKey);

  const result = await sendAndExtract(ws, 'connect', { 'db-name': opts.dbName });

  let connectionId = '';
  if (result instanceof Map) {
    connectionId = String(mapGet(result, 'database-id') ?? '');
  }

  return {
    client: c,
    dbName: opts.dbName,
    connectionId,
    _ws: ws,
  };
}

/**
 * Get the current database value.
 * Drop-in replacement for datomic.client.api/db.
 */
export async function db(conn: PgMentatConnection): Promise<PgMentatDb> {
  const result = await sendAndExtract(conn._ws, 'db', { 'db-name': conn.dbName });

  let t = 0;
  let nextT = 0;
  let databaseId = '';
  if (result instanceof Map) {
    t = mapGet(result, 't') ?? 0;
    nextT = mapGet(result, 'next-t') ?? 0;
    databaseId = String(mapGet(result, 'database-id') ?? '');
  }

  return {
    connection: conn,
    dbName: conn.dbName,
    databaseId,
    t,
    nextT,
  };
}

/**
 * Execute a Datalog query.
 * Drop-in replacement for datomic.client.api/q.
 */
export async function q(
  query: string,
  database: PgMentatDb,
  ...inputs: any[]
): Promise<any> {
  const args: Record<string, any> = {
    query,
    args: inputs,
  };
  if (database.asOfT !== undefined) args['as-of'] = database.asOfT;
  if (database.sinceT !== undefined) args['since'] = database.sinceT;
  if (database.isHistory) args['history'] = true;

  return sendAndExtract(database.connection._ws, 'q', args);
}

/**
 * Execute a transaction.
 * Drop-in replacement for datomic.client.api/transact.
 */
export async function transact(
  conn: PgMentatConnection,
  opts: { txData: string }
): Promise<any> {
  return sendAndExtract(conn._ws, 'transact', {
    'connection-id': conn.connectionId,
    'tx-data': opts.txData,
  });
}

/**
 * Pull entity attributes.
 * Drop-in replacement for datomic.client.api/pull.
 */
export async function pull(
  database: PgMentatDb,
  pattern: string,
  eid: number
): Promise<any> {
  return sendAndExtract(database.connection._ws, 'pull', {
    pattern,
    'entity-id': eid,
  });
}

/**
 * Pull attributes for multiple entities.
 */
export async function pullMany(
  database: PgMentatDb,
  pattern: string,
  eids: number[]
): Promise<any[]> {
  return Promise.all(eids.map((eid) => pull(database, pattern, eid)));
}

/**
 * Access raw datoms from an index.
 * Drop-in replacement for datomic.client.api/datoms.
 */
export async function datoms(
  database: PgMentatDb,
  opts: { index: string; components?: string[] }
): Promise<any> {
  return sendAndExtract(database.connection._ws, 'datoms', {
    index: opts.index,
    components: opts.components ?? [],
  });
}

/**
 * Speculative transaction (d/with).
 */
export async function withDb(
  database: PgMentatDb,
  opts: { txData: string }
): Promise<any> {
  return sendAndExtract(database.connection._ws, 'with', {
    'tx-data': opts.txData,
  });
}

/**
 * Query the transaction log.
 */
export async function txRange(
  conn: PgMentatConnection,
  opts?: { start?: number; end?: number }
): Promise<any> {
  const args: Record<string, any> = {};
  if (opts?.start !== undefined) args.start = opts.start;
  if (opts?.end !== undefined) args.end = opts.end;
  return sendAndExtract(conn._ws, 'tx-range', args);
}

// ---------------------------------------------------------------------------
// Time-travel database values
// ---------------------------------------------------------------------------

/**
 * Return a database value as of a specific transaction.
 */
export function asOf(database: PgMentatDb, t: number): PgMentatDb {
  return { ...database, asOfT: t, sinceT: undefined, isHistory: false };
}

/**
 * Return a database value showing only changes since a transaction.
 */
export function since(database: PgMentatDb, t: number): PgMentatDb {
  return { ...database, asOfT: undefined, sinceT: t, isHistory: false };
}

/**
 * Return a database value including all history.
 */
export function history(database: PgMentatDb): PgMentatDb {
  return { ...database, asOfT: undefined, sinceT: undefined, isHistory: true };
}

// ---------------------------------------------------------------------------
// Catalog operations
// ---------------------------------------------------------------------------

/**
 * List available databases.
 */
export async function listDatabases(c: PgMentatClient): Promise<any> {
  const ws = new WsConnection();
  await ws.connect(c.config.endpoint, c.config.apiKey);
  try {
    return await sendAndExtract(ws, 'list-dbs', {});
  } finally {
    ws.close();
  }
}

/**
 * Create a new database.
 */
export async function createDatabase(
  c: PgMentatClient,
  opts: { dbName: string }
): Promise<any> {
  const ws = new WsConnection();
  await ws.connect(c.config.endpoint, c.config.apiKey);
  try {
    return await sendAndExtract(ws, 'create-db', { 'db-name': opts.dbName });
  } finally {
    ws.close();
  }
}

/**
 * Delete a database.
 */
export async function deleteDatabase(
  c: PgMentatClient,
  opts: { dbName: string }
): Promise<any> {
  const ws = new WsConnection();
  await ws.connect(c.config.endpoint, c.config.apiKey);
  try {
    return await sendAndExtract(ws, 'delete-db', { 'db-name': opts.dbName });
  } finally {
    ws.close();
  }
}

/**
 * Release a connection (close the WebSocket).
 */
export function release(conn: PgMentatConnection): void {
  conn._ws.close();
}
