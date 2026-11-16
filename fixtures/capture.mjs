// Take one live answer per operation, for the shared deserialisation tests.
//
//   TRACEFAST_FIXTURE_BASE=<base URL, up to /v1> node fixtures/capture.mjs [--missing]
//
// The base is given in the environment and never written into the repository.
// Paged operations are called with limit=2; arrays longer than two are cut to
// two, so a fixture shows the shape, not the data.
import { readFileSync, writeFileSync, existsSync } from 'node:fs'
import { loadIr } from '../codegen/ir.mjs'

const base = process.env.TRACEFAST_FIXTURE_BASE
if (!base) throw new Error('set TRACEFAST_FIXTURE_BASE')
const headers = process.env.TRACEFAST_API_KEY ? { 'x-api-key': process.env.TRACEFAST_API_KEY } : {}
const samples = JSON.parse(readFileSync(new URL('samples.json', import.meta.url), 'utf8'))
const trim = (v) => (Array.isArray(v) ? v.slice(0, 2).map(trim) : v && typeof v === 'object' ? Object.fromEntries(Object.entries(v).map(([k, x]) => [k, trim(x)])) : v)

let failed = 0
for (const op of loadIr().operations) {
  const file = new URL(`${op.id}.json`, import.meta.url)
  if (process.argv.includes('--missing') && existsSync(file)) continue
  const own = samples.operations[op.id] ?? {}
  const value = (n) => own.path?.[n] ?? samples.byNamespace[op.namespace]?.[n] ?? samples.defaults[n]
  const path = op.path.replace('{chainId}', '1').replace(/\{(\w+)\}/g, (_, n) => encodeURIComponent(value(n)))
  const query = new URLSearchParams({ ...(op.queryParams.some((q) => q.name === 'limit') ? { limit: samples.limit } : {}), ...(own.query ?? {}) })
  const url = `${base}${path}${query.size ? `?${query}` : ''}`
  const res = await fetch(url, { headers })
  if (res.status !== 200) {
    console.log(`✗ ${op.id}: ${res.status} ${(await res.text()).slice(0, 160)}`)
    failed++
    continue
  }
  const body = trim(await res.json())
  if (body.meta) body.meta.timestamp = 1790000000000 // fixed, so a re-capture does not churn every file
  writeFileSync(file, JSON.stringify(body, null, 2) + '\n')
}
console.log(failed ? `${failed} operation(s) without a fixture` : 'every operation has a fixture')
process.exit(failed ? 1 : 0)
