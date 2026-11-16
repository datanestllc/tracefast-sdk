import { test } from 'node:test'
import assert from 'node:assert/strict'
import { TraceFast } from '../src/index.ts'
import { ir, samples, fixture, validate, mockFetch, json } from './helpers.ts'


for (const op of ir.operations) {
  test(`${op.namespace ?? 'client'}.${op.name} — request, envelope and fixture shape`, async () => {
    const body = fixture(op.id)
    const { fetch, calls } = mockFetch([json(body)])
    const tf = new TraceFast({ apiKey: 'tf_test_key', fetch, maxRetries: 0 })
    const own = samples.operations[op.id] ?? {}
    const value = (n: string) => own.path?.[n] ?? samples.byNamespace[op.namespace]?.[n] ?? samples.defaults[n]
    const args: unknown[] = op.pathParams.map((p: any) => value(p.name))
    const query = { ...(op.queryParams.some((q: any) => q.name === 'limit') ? { limit: Number(samples.limit) } : {}), ...(own.query ?? {}) }
    if (op.queryParams.length) args.push(query)
    const target: any = op.namespace ? (tf as any)[op.namespace] : tf
    const res = await target[op.name](...args)

    assert.deepEqual(res, body, 'the envelope comes back whole')
    const url = new URL(calls[0]!.url)
    const path = op.path.replace('{chainId}', '1').replace(/\{(\w+)\}/g, (_: string, n: string) => encodeURIComponent(value(n)))
    assert.equal(url.pathname, '/api/v1' + path)
    assert.deepEqual(Object.fromEntries(url.searchParams), Object.fromEntries(Object.entries(query).map(([k, v]) => [k, String(v)])), 'only the given parameters are sent')
    assert.equal(calls[0]!.headers['x-api-key'], 'tf_test_key')
    assert.match(calls[0]!.headers['user-agent']!, /^tracefast-typescript\/\d/)

    const problems: string[] = []
    validate(body, op.response, op.id, problems)
    assert.deepEqual(problems, [], 'the fixture matches the catalogue')
  })
}
