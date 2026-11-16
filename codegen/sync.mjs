// Refresh the catalogue snapshot the SDKs are generated from.
//
//   node codegen/sync.mjs
//
// openapi/openapi.json — the published REST catalogue, as served.
// openapi/stability.json — operationId → "stable" | "beta". Stability is not in
//   the published catalogue; it is read from the document the API gateway routes
//   by, whose address is given in TRACEFAST_STABILITY_SOURCE and never written
//   into this repository. Only the operation ids and their stability are kept.
import { writeFileSync } from 'node:fs'

const CATALOGUE = 'https://tracefast.xyz/api/v1/openapi.json'
const source = process.env.TRACEFAST_STABILITY_SOURCE
if (!source) throw new Error('set TRACEFAST_STABILITY_SOURCE to the URL of the routing document')

const get = async (url) => {
  const res = await fetch(url, { headers: { 'user-agent': 'tracefast-sdk-codegen' } })
  if (!res.ok) throw new Error(`${url}: HTTP ${res.status}`)
  return res.json()
}
const ops = (doc) => Object.values(doc.paths).flatMap((p) => Object.values(p)).filter((o) => o?.operationId)

const pub = await get(CATALOGUE)
const internal = await get(source)
const stability = Object.fromEntries(ops(internal).map((o) => [o.operationId, o['x-stability']]))
const out = {}
for (const op of ops(pub)) {
  const s = stability[op.operationId]
  if (s !== 'stable' && s !== 'beta') throw new Error(`${op.operationId}: stability ${s} unknown`)
  out[op.operationId] = s
}
const root = new URL('../openapi/', import.meta.url)
writeFileSync(new URL('openapi.json', root), JSON.stringify(pub, null, 2) + '\n')
writeFileSync(new URL('stability.json', root), JSON.stringify(out, Object.keys(out).sort(), 2) + '\n')
console.log(`catalogue ${pub.info.version}: ${Object.keys(out).length} operations, ${Object.values(out).filter((s) => s === 'beta').length} beta`)
