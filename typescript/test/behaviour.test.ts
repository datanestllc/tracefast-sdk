import { test } from 'node:test'
import assert from 'node:assert/strict'
import { TraceFast, TraceFastError } from '../src/index.ts'
import { mockFetch, json } from './helpers.ts'

const USDC = '0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48'
const client = (answers: Array<() => Response>, maxRetries = 2) => {
  const m = mockFetch(answers)
  return { tf: new TraceFast({ apiKey: 'tf_test_key', fetch: m.fetch, maxRetries }), calls: m.calls }
}

test('a refusal becomes TraceFastError with code, reason, details and requestId', async () => {
  const { tf } = client([json({ error: { code: 'block_not_available', message: 'past the covered history', details: { reason: 'block_not_available', maxAvailableBlock: 25901228 }, requestId: 'req-1' } }, 422)])
  const e = await tf.addresses.portfolio(USDC, { block: 99999999 }).catch((x) => x)
  assert.ok(e instanceof TraceFastError)
  assert.equal(e.status, 422)
  assert.equal(e.code, 'block_not_available')
  assert.equal(e.reason, 'block_not_available')
  assert.equal(e.details?.maxAvailableBlock, 25901228)
  assert.equal(e.requestId, 'req-1')
})

test('a body that is not the error envelope is unexpected_response', async () => {
  const { tf } = client([() => new Response('<html>bad gateway</html>', { status: 502 })])
  const e = await tf.blocks.latest().catch((x) => x)
  assert.equal(e.code, 'unexpected_response')
  assert.equal(e.status, 502)
})

test('the key never appears in an error', async () => {
  const { tf } = client([json({ error: { code: 'not_found', message: 'no such block' } }, 404)])
  const e = await tf.blocks.get('1').catch((x) => x)
  assert.ok(!JSON.stringify({ ...e, message: e.message }).includes('tf_test_key'))
})

test('429 with Retry-After is retried; without it, returned at once', async () => {
  const ok = json({ data: { number: 1 }, meta: {} })
  const limited = json({ error: { code: 'rate_limited', message: 'slow down' } }, 429, { 'retry-after': '0' })
  const r1 = client([limited, ok])
  assert.deepEqual((await r1.tf.blocks.latest()).data, { number: 1 })
  assert.equal(r1.calls.length, 2)

  const quota = json({ error: { code: 'quota_exceeded', message: 'daily quota' } }, 429)
  const r2 = client([quota, ok])
  await assert.rejects(r2.tf.blocks.latest(), (e: TraceFastError) => e.code === 'quota_exceeded')
  assert.equal(r2.calls.length, 1)
})

test('503 is retried, 400/404/422/500 are not', async () => {
  const ok = json({ data: {}, meta: {} })
  const r1 = client([json({ error: { code: 'service_unavailable', message: 'busy' } }, 503, { 'retry-after': '0' }), ok])
  await r1.tf.blocks.latest()
  assert.equal(r1.calls.length, 2)
  for (const status of [400, 404, 422, 500]) {
    const r = client([json({ error: { code: 'x', message: 'y' } }, status), ok])
    await assert.rejects(r.tf.blocks.latest())
    assert.equal(r.calls.length, 1, `status ${status}`)
  }
})

test('retries stop at maxRetries', async () => {
  const busy = json({ error: { code: 'service_unavailable', message: 'busy' } }, 503, { 'retry-after': '0' })
  const r = client([busy, busy, busy, busy], 2)
  await assert.rejects(r.tf.blocks.latest(), (e: TraceFastError) => e.status === 503)
  assert.equal(r.calls.length, 3)
})

test('a network failure becomes connection_error after the retries', async () => {
  const boom = () => {
    throw new TypeError('fetch failed')
  }
  const r = client([boom, boom], 1)
  await assert.rejects(r.tf.blocks.latest(), (e: TraceFastError) => e.code === 'connection_error' && e.status === 0)
})

test('pages follow nextCursor with the same parameters and stop when it is absent', async () => {
  const p1 = json({ data: { transactions: [1, 2], nextCursor: 'c1' }, meta: {} })
  const p2 = json({ data: { transactions: [3] }, meta: {} })
  const { tf, calls } = client([p1, p2])
  const pages = []
  for await (const page of tf.addresses.transactionsPages(USDC, { limit: 2, direction: 'in' })) pages.push(page)
  assert.equal(pages.length, 2)
  const second = new URL(calls[1]!.url)
  assert.equal(second.searchParams.get('cursor'), 'c1')
  assert.equal(second.searchParams.get('limit'), '2')
  assert.equal(second.searchParams.get('direction'), 'in')
})

test('a short page with a nextCursor is not the last one', async () => {
  const p1 = json({ data: { transactions: [1], nextCursor: 'c1' }, meta: {} })
  const p2 = json({ data: { transactions: [] }, meta: {} })
  const { tf } = client([p1, p2])
  let n = 0
  for await (const _ of tf.addresses.transactionsPages(USDC, { limit: 50 })) n++
  assert.equal(n, 2)
})

test('chainId comes from the client', async () => {
  const { fetch, calls } = mockFetch([json({ data: {}, meta: {} })])
  await new TraceFast({ apiKey: 'k', fetch, chainId: 1 }).blocks.latest()
  assert.match(new URL(calls[0]!.url).pathname, /^\/api\/v1\/chains\/1\/blocks\/latest$/)
})

test('apiKey is required', () => {
  assert.throws(() => new TraceFast({ apiKey: '' }), TypeError)
})

test('a redirect is not followed — it is an unexpected_response', async () => {
  const { tf, calls } = client([() => new Response(null, { status: 302, headers: { location: 'https://elsewhere.example/' } })])
  await assert.rejects(tf.blocks.latest(), (e: TraceFastError) => e.code === 'unexpected_response' && e.status === 302)
  assert.equal(calls.length, 1)
})

test('a 2xx body that is not JSON is an unexpected_response', async () => {
  const { tf } = client([() => new Response('<html>ok</html>', { status: 200 })])
  await assert.rejects(tf.blocks.latest(), (e: TraceFastError) => e.code === 'unexpected_response' && e.status === 200)
})
