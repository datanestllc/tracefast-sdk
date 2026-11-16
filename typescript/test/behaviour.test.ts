import { test } from 'node:test'
import assert from 'node:assert/strict'
import { inspect } from 'node:util'
import {
  TraceFast,
  TraceFastError,
  AuthenticationError,
  PermissionDeniedError,
  NotFoundError,
  RateLimitError,
  UnprocessableEntityError,
  InternalServerError,
  APIConnectionError,
  APITimeoutError,
  UnexpectedResponseError,
  RequestAbortedError,
} from '../src/index.ts'
import { mockFetch, json } from './helpers.ts'

const USDC = '0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48'
const KEY = 'tf_test_key'
const client = (answers: Array<() => Response>, options: Record<string, unknown> = {}) => {
  const m = mockFetch(answers)
  return { tf: new TraceFast({ apiKey: KEY, fetch: m.fetch, ...options }), calls: m.calls }
}
const ok = json({ data: { number: 1 }, meta: {} })
const fail = (status: number, error: Record<string, unknown>, headers: Record<string, string> = {}) => json({ error }, status, headers)

// ---- errors ------------------------------------------------------------

test('a refusal carries status, code, reason, details, requestId and the raw body', async () => {
  const { tf } = client([fail(422, { code: 'block_not_available', message: 'past the covered history', details: { reason: 'block_not_available', maxAvailableBlock: 25901228 }, requestId: 'req-1' })])
  const e = await tf.addresses.portfolio(USDC, { block: 99999999 }).catch((x) => x)
  assert.ok(e instanceof UnprocessableEntityError && e instanceof TraceFastError)
  assert.equal(e.status, 422)
  assert.equal(e.code, 'block_not_available')
  assert.equal(e.reason, 'block_not_available')
  assert.equal(e.details?.maxAvailableBlock, 25901228)
  assert.equal(e.requestId, 'req-1')
  assert.equal(e.body?.code, 'block_not_available')
  assert.equal(e.message, '422 block_not_available: past the covered history')
})

test("the gateway's top-level reason and bucket are read", async () => {
  const { tf } = client([fail(401, { code: 'auth_failed', message: 'missing key', reason: 'api_key_required' }, { 'x-request-id': 'hdr-1' })])
  const e = await tf.blocks.latest().catch((x) => x)
  assert.ok(e instanceof AuthenticationError)
  assert.equal(e.reason, 'api_key_required')
  assert.equal(e.requestId, 'hdr-1', 'requestId falls back to the X-Request-Id header')

  const r = client([fail(429, { code: 'rate_limited', message: 'slow down', bucket: 'base' }, { 'retry-after': '120' })])
  const e2 = await r.tf.blocks.latest().catch((x) => x)
  assert.ok(e2 instanceof RateLimitError)
  assert.equal(e2.bucket, 'base')

  const p = client([fail(403, { code: 'forbidden', message: 'plan', reason: 'plan_scope_missing', details: { minimumPlan: 'intelligence', currentPlan: 'data' } })])
  const e3 = await p.tf.contracts.capabilities(USDC).catch((x) => x)
  assert.ok(e3 instanceof PermissionDeniedError)
  assert.equal(e3.reason, 'plan_scope_missing')
  assert.equal(e3.details?.minimumPlan, 'intelligence')
})

test('the subclass follows the status', async () => {
  const cases: Array<[number, Function]> = [[404, NotFoundError], [500, InternalServerError], [503, InternalServerError]]
  for (const [status, Cls] of cases) {
    const { tf } = client([fail(status, { code: 'x', message: 'y' })], { maxRetries: 0 })
    const e = await tf.blocks.latest().catch((x) => x)
    assert.ok(e instanceof Cls, `status ${status}`)
    assert.ok(TraceFastError.is(e))
  }
})

test('a body that is not the error envelope is unexpected_response', async () => {
  const { tf } = client([() => new Response('<html>bad gateway</html>', { status: 502 })], { maxRetries: 0 })
  const e = await tf.blocks.latest().catch((x) => x)
  assert.ok(e instanceof UnexpectedResponseError)
  assert.equal(e.status, 502)
})

test('a redirect is never followed, even with an error body', async () => {
  const { tf, calls } = client([fail(302, { code: 'moved', message: 'go elsewhere' }, { location: 'https://elsewhere.example/' })])
  const e = await tf.blocks.latest().catch((x) => x)
  assert.ok(e instanceof UnexpectedResponseError)
  assert.equal(e.status, 302)
  assert.equal(calls.length, 1)
})

test('a 2xx body that is not JSON is unexpected_response', async () => {
  const { tf } = client([() => new Response('<html>ok</html>', { status: 200 })])
  await assert.rejects(tf.blocks.latest(), (e: TraceFastError) => e instanceof UnexpectedResponseError && e.status === 200)
})

// ---- the key -------------------------------------------------------------

test('a key with a character a header cannot carry is refused at construction, without echoing it', () => {
  for (const bad of ['', 'tf_live_SEC\nRET', 'tf_live_SEC RET', 'tf_live_…', ' tf_live_x']) {
    let err: unknown
    try {
      new TraceFast({ apiKey: bad })
    } catch (e) {
      err = e
    }
    assert.ok(err instanceof TypeError, JSON.stringify(bad))
    if (bad.length > 3) assert.ok(!(err as Error).message.includes(bad.trim()))
  }
})

test('the key appears in no error, inspect or JSON output', async () => {
  const { tf } = client([fail(404, { code: 'not_found', message: 'no such block' })])
  const e = await tf.blocks.get(1).catch((x) => x)
  for (const s of [e.message, inspect(e, { depth: 5 }), JSON.stringify(e), inspect(tf, { depth: 5 }), JSON.stringify(tf)]) assert.ok(!s.includes(KEY))
})

test('baseUrl must be https, except on a loopback host', () => {
  assert.throws(() => new TraceFast({ apiKey: KEY, baseUrl: 'http://example.com/api/v1' }), TypeError)
  assert.throws(() => new TraceFast({ apiKey: KEY, baseUrl: 'https://example.com/api/v1#x' }), TypeError)
  new TraceFast({ apiKey: KEY, baseUrl: 'http://localhost/api/v1' })
})

// ---- requests ------------------------------------------------------------

test('a path value of "", "." or ".." is refused before any request', async () => {
  const { tf, calls } = client([ok])
  for (const v of ['', '.', '..']) await assert.rejects(tf.contracts.proxy(v), TypeError)
  assert.equal(calls.length, 0)
})

test('chainId comes from the client; integer path values accept numbers', async () => {
  const { tf, calls } = client([ok])
  await tf.blocks.get(25920000)
  assert.equal(new URL(calls[0]!.url).pathname, '/api/v1/chains/1/blocks/25920000')
  assert.equal(calls[0]!.headers.accept, 'application/json')
})

test('the user agent names the SDK and the suffix', async () => {
  const { tf, calls } = client([ok], { userAgent: 'my-app/1.2' })
  await tf.blocks.latest()
  assert.match(calls[0]!.headers['user-agent']!, /^tracefast-typescript\/\d+\.\d+\.\d+ my-app\/1\.2$/)
})

test('withOptions keeps the key and changes the rest', async () => {
  const { tf, calls } = client([ok])
  await tf.withOptions({ userAgent: 'other/1' }).blocks.latest()
  assert.equal(calls[0]!.headers['x-api-key'], KEY)
  assert.match(calls[0]!.headers['user-agent']!, / other\/1$/)
})

// ---- retries -------------------------------------------------------------

test('429 with a short Retry-After is retried', async () => {
  const r = client([fail(429, { code: 'rate_limited', message: 'slow down', bucket: 'base' }, { 'retry-after': '0' }), ok])
  assert.deepEqual((await r.tf.blocks.latest()).data, { number: 1 })
  assert.equal(r.calls.length, 2)
})

test('429 without Retry-After, with a long one, or quota_exceeded is returned at once', async () => {
  const cases: Array<[Record<string, unknown>, Record<string, string>]> = [
    [{ code: 'concurrency_limited', message: 'busy', bucket: 'concurrency' }, {}],
    [{ code: 'rate_limited', message: 'daily', bucket: 'quota_method' }, { 'retry-after': '3600' }],
    [{ code: 'quota_exceeded', message: 'decompilations', details: { reason: 'decompile_in_flight_limit' } }, { 'retry-after': '0' }],
  ]
  for (const [error, headers] of cases) {
    const r = client([fail(429, error, headers), ok])
    const e = await r.tf.blocks.latest().catch((x) => x)
    assert.ok(e instanceof RateLimitError)
    assert.equal(r.calls.length, 1, String(error.code))
    if (headers['retry-after'] !== undefined) assert.equal(e.retryAfter, Number(headers['retry-after']), 'retryAfter is exactly as sent')
  }
})

test('maxRetryWait sets the longest wait the client accepts', async () => {
  const r = client([fail(429, { code: 'rate_limited', message: 'x' }, { 'retry-after': '1' }), ok], { maxRetryWait: 0 })
  await assert.rejects(r.tf.blocks.latest(), RateLimitError)
  assert.equal(r.calls.length, 1)
})

test('503 is retried; 400/404/422/500 are not', async () => {
  const r1 = client([fail(503, { code: 'service_unavailable', message: 'busy' }, { 'retry-after': '0' }), ok])
  await r1.tf.blocks.latest()
  assert.equal(r1.calls.length, 2)
  for (const status of [400, 404, 422, 500]) {
    const r = client([fail(status, { code: 'x', message: 'y' }), ok])
    await assert.rejects(r.tf.blocks.latest())
    assert.equal(r.calls.length, 1, `status ${status}`)
  }
})

test('retries stop at maxRetries, per client or per call', async () => {
  const busy = fail(503, { code: 'service_unavailable', message: 'busy' }, { 'retry-after': '0' })
  const r = client([busy, busy, busy, busy])
  await assert.rejects(r.tf.blocks.latest(), (e: TraceFastError) => e.status === 503)
  assert.equal(r.calls.length, 3)
  const r2 = client([busy, busy])
  await assert.rejects(r2.tf.blocks.latest({ maxRetries: 0 }))
  assert.equal(r2.calls.length, 1)
})

test('a connection failure is retried, then becomes APIConnectionError', async () => {
  const boom = () => {
    throw new TypeError('fetch failed')
  }
  const r = client([boom, ok])
  await r.tf.blocks.latest()
  assert.equal(r.calls.length, 2)
  const r2 = client([boom, boom], { maxRetries: 1 })
  await assert.rejects(r2.tf.blocks.latest(), (e: TraceFastError) => e instanceof APIConnectionError && e.status === 0 && e.cause instanceof TypeError)
})

test('a body cut off while reading is a connection error, and retried', async () => {
  const broken = () =>
    new Response(new ReadableStream({ start: (c) => (c.enqueue(new TextEncoder().encode('{"data"')), c.error(new TypeError('terminated'))) }), { status: 200 })
  const r = client([broken, ok])
  await r.tf.blocks.latest()
  assert.equal(r.calls.length, 2)
})

test('a timeout is APITimeoutError and is not retried', async () => {
  // A body that never ends; like a real fetch, the abort signal breaks the read.
  let calls = 0
  const fetch = (async (_: unknown, init?: RequestInit) => {
    calls++
    const body = new ReadableStream({
      start: (c) => init?.signal?.addEventListener('abort', () => c.error(init.signal!.reason)),
    })
    return new Response(body, { status: 200 })
  }) as typeof globalThis.fetch
  const tf = new TraceFast({ apiKey: KEY, fetch, timeout: 50 })
  await assert.rejects(tf.blocks.latest(), (e: TraceFastError) => e instanceof APITimeoutError)
  assert.equal(calls, 1)
})

test("the caller's abort signal becomes RequestAbortedError", async () => {
  const controller = new AbortController()
  const tf = new TraceFast({
    apiKey: KEY,
    fetch: ((_: unknown, init?: RequestInit) =>
      new Promise((_r, reject) => init?.signal?.addEventListener('abort', () => reject(init.signal!.reason)))) as typeof fetch,
  })
  const p = tf.blocks.latest({ signal: controller.signal })
  controller.abort()
  await assert.rejects(p, (e: TraceFastError) => e instanceof RequestAbortedError && e.code === 'aborted')
})

// ---- pagination ------------------------------------------------------------

test('pages follow nextCursor with the same parameters and stop when it is absent', async () => {
  const p1 = json({ data: { transactions: [1, 2], nextCursor: 'c1' }, meta: {} })
  const p2 = json({ data: { transactions: [3] }, meta: {} })
  const { tf, calls } = client([p1, p2])
  const pages = []
  for await (const page of tf.addresses.transactionsPages(USDC, { limit: 2, direction: 'sent' })) pages.push(page)
  assert.equal(pages.length, 2)
  const second = new URL(calls[1]!.url)
  assert.equal(second.searchParams.get('cursor'), 'c1')
  assert.equal(second.searchParams.get('limit'), '2')
  assert.equal(second.searchParams.get('direction'), 'sent')
})

test('a short page with a nextCursor is not the last one', async () => {
  const { tf } = client([json({ data: { transactions: [1], nextCursor: 'c1' }, meta: {} }), json({ data: { transactions: [] }, meta: {} })])
  let n = 0
  for await (const _ of tf.addresses.transactionsPages(USDC, { limit: 50 })) n++
  assert.equal(n, 2)
})

test('items walk the rows across pages, and stop at maxItems', async () => {
  const p1 = json({ data: { transactions: [{ hash: 'a' }, { hash: 'b' }], nextCursor: 'c1' }, meta: {} })
  const p2 = json({ data: { transactions: [{ hash: 'c' }] }, meta: {} })
  const { tf } = client([p1, p2])
  const rows = []
  for await (const tx of tf.addresses.transactionsItems(USDC)) rows.push(tx.hash)
  assert.deepEqual(rows, ['a', 'b', 'c'])

  const r = client([p1, p2])
  const few = []
  for await (const tx of r.tf.addresses.transactionsItems(USDC, {}, { maxItems: 2 })) few.push(tx.hash)
  assert.deepEqual(few, ['a', 'b'])
  assert.equal(r.calls.length, 1, 'no page is fetched beyond what maxItems needs')
})

test('maxPages stops the page walker', async () => {
  const { tf, calls } = client([json({ data: { transactions: [], nextCursor: 'c1' }, meta: {} })])
  for await (const _ of tf.addresses.transactionsPages(USDC, {}, { maxPages: 1 })) void _
  assert.equal(calls.length, 1)
})

test('an empty or repeated nextCursor is unexpected_response', async () => {
  const same = json({ data: { transactions: [], nextCursor: 'c1' }, meta: {} })
  const r = client([same, same])
  await assert.rejects(async () => {
    for await (const _ of r.tf.addresses.transactionsPages(USDC)) void _
  }, (e: TraceFastError) => e instanceof UnexpectedResponseError)
  const r2 = client([json({ data: { transactions: [], nextCursor: '' }, meta: {} })])
  await assert.rejects(async () => {
    for await (const _ of r2.tf.addresses.transactionsPages(USDC)) void _
  }, (e: TraceFastError) => e instanceof UnexpectedResponseError)
})

test('a walker can start from a given cursor', async () => {
  const { tf, calls } = client([json({ data: { transactions: [] }, meta: {} })])
  for await (const _ of tf.addresses.transactionsPages(USDC, { cursor: 'start' })) void _
  assert.equal(new URL(calls[0]!.url).searchParams.get('cursor'), 'start')
})

// ---- construction ------------------------------------------------------------

test('bad numeric options are refused', () => {
  assert.throws(() => new TraceFast({ apiKey: KEY, timeout: Infinity }), RangeError)
  assert.throws(() => new TraceFast({ apiKey: KEY, maxRetries: -1 }), RangeError)
})

test('VERSION matches package.json', async () => {
  const { VERSION } = await import('../src/index.ts')
  const pkg = JSON.parse(await import('node:fs').then((fs) => fs.readFileSync(new URL('../package.json', import.meta.url), 'utf8')))
  assert.equal(VERSION, pkg.version)
})
