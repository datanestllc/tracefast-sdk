// Run against the BUILT package (dist/), as a consumer would load it: import,
// construct, one mocked call. CI runs it on every supported Node version.
import assert from 'node:assert/strict'
import { TraceFast, TraceFastError, NotFoundError, VERSION, CATALOGUE_VERSION } from '../dist/index.js'

const tf = new TraceFast({
  apiKey: 'tf_test_key',
  fetch: async () => new Response(JSON.stringify({ error: { code: 'not_found', message: 'no such block' } }), { status: 404 }),
})
const e = await tf.blocks.get(1).catch((x) => x)
assert.ok(e instanceof NotFoundError && e instanceof TraceFastError)
assert.match(VERSION, /^\d+\.\d+\.\d+/)
assert.ok(CATALOGUE_VERSION)
console.log(`smoke ok: @tracefast_xyz/sdk ${VERSION}, catalogue ${CATALOGUE_VERSION}, node ${process.versions.node}`)
