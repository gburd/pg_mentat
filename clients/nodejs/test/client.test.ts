/**
 * Unit and integration tests for the pg_mentat TypeScript client.
 *
 * Unit tests (transit encoding/decoding) run without a server.
 * Integration tests require a running mentatd instance.
 */

import {
  Keyword,
  Symbol,
  kw,
  PgMentatError,
  client,
  asOf,
  since,
  history,
} from '../src/client';

// We need access to internal functions for testing.
// Re-implement the transit helpers inline since they are not exported.
// In a real test setup you'd either export them or use a test build.

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
  if (Array.isArray(v)) return v.map(transitEncodeValue);
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
  if (s.startsWith('~:')) return new Keyword(s.slice(2));
  if (s.startsWith('~$')) return new Symbol(s.slice(2));
  if (s.startsWith('~i')) return parseInt(s.slice(2), 10);
  if (s.startsWith('~u')) return s.slice(2);
  if (s.startsWith('~m')) return new Date(parseInt(s.slice(2), 10));
  if (s === '~zNaN') return NaN;
  if (s === '~zINF') return Infinity;
  if (s === '~z-INF') return -Infinity;
  if (s.startsWith('~~')) return s.slice(1);
  if (s.startsWith('~^')) return '^' + s.slice(2);
  return s;
}

function transitDecode(v: any): any {
  if (v === null || v === undefined) return null;
  if (typeof v === 'boolean' || typeof v === 'number') return v;
  if (typeof v === 'string') return transitDecodeTagged(v);
  if (Array.isArray(v)) {
    if (v.length > 0 && v[0] === '^ ') {
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
  return v;
}

function parseTransitJson(s: string): any {
  return transitDecode(JSON.parse(s));
}

function mapGet(m: Map<any, any>, keyName: string): any {
  for (const [k, v] of m.entries()) {
    if (k instanceof Keyword && k.toString() === keyName) return v;
    if (typeof k === 'string' && k === keyName) return v;
  }
  return undefined;
}

// ---------------------------------------------------------------------------
// Keyword / Symbol types
// ---------------------------------------------------------------------------

describe('Keyword', () => {
  test('simple keyword', () => {
    const k = new Keyword('name');
    expect(k.name).toBe('name');
    expect(k.namespace).toBeNull();
    expect(k.toString()).toBe('name');
  });

  test('namespaced keyword from string', () => {
    const k = new Keyword('person/name');
    expect(k.name).toBe('name');
    expect(k.namespace).toBe('person');
    expect(k.toString()).toBe('person/name');
  });

  test('explicit namespace', () => {
    const k = new Keyword('name', 'person');
    expect(k.name).toBe('name');
    expect(k.namespace).toBe('person');
  });

  test('equality', () => {
    const a = new Keyword('person/name');
    const b = new Keyword('name', 'person');
    expect(a.equals(b)).toBe(true);
  });
});

describe('Symbol', () => {
  test('creation', () => {
    const s = new Symbol('?e');
    expect(s.name).toBe('?e');
    expect(s.toString()).toBe('?e');
  });
});

describe('kw helper', () => {
  test('creates keyword', () => {
    const k = kw('db/name');
    expect(k).toBeInstanceOf(Keyword);
    expect(k.toString()).toBe('db/name');
  });
});

// ---------------------------------------------------------------------------
// Transit encoding
// ---------------------------------------------------------------------------

describe('Transit encoding', () => {
  test('null', () => {
    expect(JSON.parse(transitEncode(null))).toBeNull();
  });

  test('boolean', () => {
    expect(JSON.parse(transitEncode(true))).toBe(true);
    expect(JSON.parse(transitEncode(false))).toBe(false);
  });

  test('small integer', () => {
    expect(JSON.parse(transitEncode(42))).toBe(42);
  });

  test('large integer as tagged string', () => {
    const encoded = transitEncode(9_999_999_999);
    expect(encoded).toContain('~i9999999999');
  });

  test('float', () => {
    expect(JSON.parse(transitEncode(3.14))).toBeCloseTo(3.14);
  });

  test('plain string', () => {
    expect(JSON.parse(transitEncode('hello'))).toBe('hello');
  });

  test('string with tilde escaped', () => {
    const encoded = transitEncode('~special');
    expect(encoded).toContain('~~special');
  });

  test('string with caret escaped', () => {
    const encoded = transitEncode('^start');
    expect(encoded).toContain('~^start');
  });

  test('keyword', () => {
    const encoded = transitEncode(new Keyword('person/name'));
    expect(encoded).toContain('~:person/name');
  });

  test('symbol', () => {
    const encoded = transitEncode(new Symbol('?e'));
    expect(encoded).toContain('~$?e');
  });

  test('array', () => {
    expect(JSON.parse(transitEncode([1, 2, 3]))).toEqual([1, 2, 3]);
  });

  test('object as cmap', () => {
    const encoded = JSON.parse(transitEncode({ name: 'Alice' }));
    expect(encoded[0]).toBe('^ ');
    expect(encoded).toContain('name');
    expect(encoded).toContain('Alice');
  });
});

// ---------------------------------------------------------------------------
// Transit decoding
// ---------------------------------------------------------------------------

describe('Transit decoding', () => {
  test('null', () => {
    expect(transitDecode(null)).toBeNull();
  });

  test('boolean', () => {
    expect(transitDecode(true)).toBe(true);
    expect(transitDecode(false)).toBe(false);
  });

  test('number', () => {
    expect(transitDecode(42)).toBe(42);
    expect(transitDecode(3.14)).toBe(3.14);
  });

  test('plain string', () => {
    expect(transitDecode('hello')).toBe('hello');
  });

  test('keyword', () => {
    const result = transitDecode('~:person/name');
    expect(result).toBeInstanceOf(Keyword);
    expect(result.namespace).toBe('person');
    expect(result.name).toBe('name');
  });

  test('simple keyword', () => {
    const result = transitDecode('~:name');
    expect(result).toBeInstanceOf(Keyword);
    expect(result.name).toBe('name');
  });

  test('symbol', () => {
    const result = transitDecode('~$?e');
    expect(result).toBeInstanceOf(Symbol);
    expect(result.name).toBe('?e');
  });

  test('large integer', () => {
    expect(transitDecode('~i9999999999')).toBe(9999999999);
  });

  test('uuid', () => {
    expect(transitDecode('~u550e8400-e29b-41d4-a716-446655440000'))
      .toBe('550e8400-e29b-41d4-a716-446655440000');
  });

  test('instant', () => {
    const result = transitDecode('~m1714000000000');
    expect(result).toBeInstanceOf(Date);
    expect((result as Date).getTime()).toBe(1714000000000);
  });

  test('NaN', () => {
    expect(transitDecode('~zNaN')).toBeNaN();
  });

  test('Infinity', () => {
    expect(transitDecode('~zINF')).toBe(Infinity);
  });

  test('-Infinity', () => {
    expect(transitDecode('~z-INF')).toBe(-Infinity);
  });

  test('escaped tilde', () => {
    expect(transitDecode('~~hello')).toBe('~hello');
  });

  test('escaped caret', () => {
    expect(transitDecode('~^hello')).toBe('^hello');
  });

  test('cmap', () => {
    const result = transitDecode(['^ ', '~:name', 'Alice', '~:age', 30]);
    expect(result).toBeInstanceOf(Map);
    expect(mapGet(result, 'name')).toBe('Alice');
    expect(mapGet(result, 'age')).toBe(30);
  });

  test('nested cmap', () => {
    const result = transitDecode([
      '^ ', '~:result',
      ['^ ', '~:db-name', 'test-db', '~:t', 1000],
    ]);
    expect(result).toBeInstanceOf(Map);
    const inner = mapGet(result, 'result');
    expect(inner).toBeInstanceOf(Map);
    expect(mapGet(inner, 'db-name')).toBe('test-db');
    expect(mapGet(inner, 't')).toBe(1000);
  });

  test('vector of vectors', () => {
    const result = transitDecode([[42, 'Alice'], [43, 'Bob']]);
    expect(result).toHaveLength(2);
    expect(result[0]).toEqual([42, 'Alice']);
  });

  test('tagged list', () => {
    const result = transitDecode(['~#list', [1, 2, 3]]);
    expect(result).toEqual([1, 2, 3]);
  });

  test('tagged set', () => {
    const result = transitDecode(['~#set', [1, 2, 3]]);
    expect(result).toBeInstanceOf(Set);
    expect(result).toEqual(new Set([1, 2, 3]));
  });
});

// ---------------------------------------------------------------------------
// Full Transit+JSON parsing
// ---------------------------------------------------------------------------

describe('parseTransitJson', () => {
  test('success response', () => {
    const result = parseTransitJson('["^ ","~:result",42]');
    expect(result).toBeInstanceOf(Map);
    expect(mapGet(result, 'result')).toBe(42);
  });

  test('error response', () => {
    const json = [
      '["^ ","~:error",',
      '["^ ",',
      '"~:cognitect.anomalies/category","~:cognitect.anomalies/not-found",',
      '"~:cognitect.anomalies/message","Database not found"]]',
    ].join('');
    const result = parseTransitJson(json);
    const error = mapGet(result, 'error');
    expect(error).toBeInstanceOf(Map);
    const cat = mapGet(error, 'cognitect.anomalies/category');
    expect(cat).toBeInstanceOf(Keyword);
    expect(cat.toString()).toBe('cognitect.anomalies/not-found');
    expect(mapGet(error, 'cognitect.anomalies/message')).toBe('Database not found');
  });

  test('query result', () => {
    const result = parseTransitJson(
      '["^ ","~:result",[[42,"Alice"],[43,"Bob"]]]'
    );
    const rows = mapGet(result, 'result');
    expect(rows).toHaveLength(2);
    expect(rows[0]).toEqual([42, 'Alice']);
  });

  test('connect response', () => {
    const json = [
      '["^ ","~:result",',
      '["^ ",',
      '"~:db-name","test-db",',
      '"~:database-id","conn-123",',
      '"~:t",1000,',
      '"~:next-t",1001,',
      '"~:type","~:datomic.client/connection"]]',
    ].join('');
    const result = parseTransitJson(json);
    const inner = mapGet(result, 'result');
    expect(mapGet(inner, 'db-name')).toBe('test-db');
    expect(mapGet(inner, 'database-id')).toBe('conn-123');
    expect(mapGet(inner, 't')).toBe(1000);
    expect(mapGet(inner, 'next-t')).toBe(1001);
  });

  test('welcome message', () => {
    const json = [
      '["^ ",',
      '"~:type","~:datomic.client/session",',
      '"~:session-id","abc-123",',
      '"~:protocol-version",1]',
    ].join('');
    const result = parseTransitJson(json);
    const type = mapGet(result, 'type');
    expect(type).toBeInstanceOf(Keyword);
    expect(type.toString()).toBe('datomic.client/session');
    expect(mapGet(result, 'session-id')).toBe('abc-123');
    expect(mapGet(result, 'protocol-version')).toBe(1);
  });
});

// ---------------------------------------------------------------------------
// Client API types
// ---------------------------------------------------------------------------

describe('client()', () => {
  test('requires endpoint', () => {
    expect(() => client({ endpoint: '' })).toThrow();
  });

  test('stores config', () => {
    const c = client({ endpoint: 'ws://localhost:8080/ws' });
    expect(c.config.endpoint).toBe('ws://localhost:8080/ws');
  });
});

describe('Time-travel db values', () => {
  const mockDb = {
    connection: {} as any,
    dbName: 'test',
    databaseId: 'id',
    t: 1000,
    nextT: 1001,
  };

  test('asOf', () => {
    const result = asOf(mockDb, 500);
    expect(result.asOfT).toBe(500);
    expect(result.sinceT).toBeUndefined();
    expect(result.isHistory).toBe(false);
  });

  test('since', () => {
    const result = since(mockDb, 500);
    expect(result.sinceT).toBe(500);
    expect(result.asOfT).toBeUndefined();
    expect(result.isHistory).toBe(false);
  });

  test('history', () => {
    const result = history(mockDb);
    expect(result.isHistory).toBe(true);
    expect(result.asOfT).toBeUndefined();
    expect(result.sinceT).toBeUndefined();
  });
});

// ---------------------------------------------------------------------------
// Error handling
// ---------------------------------------------------------------------------

describe('PgMentatError', () => {
  test('creation', () => {
    const err = new PgMentatError('not found', 'not-found');
    expect(err.message).toBe('not found');
    expect(err.category).toBe('not-found');
    expect(err.name).toBe('PgMentatError');
  });

  test('with response', () => {
    const resp = { key: 'val' };
    const err = new PgMentatError('fail', 'fault', resp);
    expect(err.response).toBe(resp);
  });
});
