// The catalogue snapshot turned into one language-neutral model that every
// emitter reads: operations with their parameters and response type, and named
// types. Inline object schemas are hoisted into named types, so every object
// an emitter meets has a name.
import { readFileSync } from 'node:fs'

const root = new URL('../', import.meta.url)
const read = (p) => JSON.parse(readFileSync(new URL(p, root), 'utf8'))

export function loadIr() {
  const doc = read('openapi/openapi.json')
  const stability = read('openapi/stability.json')
  const names = read('codegen/names.json')
  const schemas = doc.components.schemas
  const types = new Map() // name → { kind: 'object', description, fields } | { kind: 'alias', description, type }

  const pascal = (s) => s.replace(/(^|[_\-\s])(\w)/g, (_, __, c) => c.toUpperCase()).replace(/[^A-Za-z0-9]/g, '')
  const refName = (ref) => ref.split('/').pop()

  // A schema → a type reference. `hint` names a hoisted inline object.
  const typeOf = (s, hint) => {
    if (!s) return { kind: 'any' }
    if (s.$ref) {
      declare(refName(s.$ref))
      return { kind: 'ref', name: refName(s.$ref) }
    }
    if (s.oneOf || s.anyOf) {
      const alts = (s.oneOf ?? s.anyOf).filter((a) => a.type !== 'null')
      if (alts.length === 1) return typeOf(alts[0], hint)
      return { kind: 'any' }
    }
    if (s.allOf) {
      if (s.allOf.length === 1) return typeOf(s.allOf[0], hint)
      return objectType({ ...s, allOf: undefined, properties: mergedProps(s), required: mergedRequired(s) }, hint)
    }
    const t = Array.isArray(s.type) ? s.type.filter((x) => x !== 'null')[0] : s.type
    if (s.enum) return { kind: 'enum', values: s.enum.filter((v) => v !== null).map(String) }
    if (t === 'string') return { kind: 'string' }
    if (t === 'integer') return { kind: 'integer' }
    if (t === 'number') return { kind: 'number' }
    if (t === 'boolean') return { kind: 'boolean' }
    if (t === 'array') return { kind: 'array', items: typeOf(s.items, `${hint}Item`) }
    if (t === 'object' || s.properties) {
      if (!s.properties && s.additionalProperties && s.additionalProperties !== true)
        return { kind: 'map', values: typeOf(s.additionalProperties, `${hint}Value`) }
      if (!s.properties) return { kind: 'map', values: { kind: 'any' } }
      return objectType(s, hint)
    }
    return { kind: 'any' }
  }
  const nullable = (s) =>
    !!s && ((Array.isArray(s.type) && s.type.includes('null')) || (s.oneOf ?? s.anyOf ?? []).some((a) => a.type === 'null') || s.nullable === true)
  const mergedProps = (s) => Object.assign({}, ...s.allOf.map((a) => (a.$ref ? schemas[refName(a.$ref)] : a).properties ?? {}), s.properties ?? {})
  const mergedRequired = (s) => [...new Set(s.allOf.flatMap((a) => (a.$ref ? schemas[refName(a.$ref)] : a).required ?? []).concat(s.required ?? []))]
  const describe = (p) => p?.description ?? (p?.allOf ?? p?.oneOf ?? []).map((a) => a.description).find(Boolean) ?? ''

  const objectType = (s, name) => {
    if (!name) throw new Error('inline object without a name')
    let unique = name
    for (let i = 2; types.has(unique) && types.get(unique).source !== s; i++) unique = `${name}${i}`
    if (types.has(unique)) return { kind: 'ref', name: unique }
    const entry = { kind: 'object', description: s.description ?? '', fields: [], source: s }
    types.set(unique, entry)
    const req = new Set(s.required ?? [])
    for (const [field, ps] of Object.entries(s.properties ?? {})) {
      entry.fields.push({
        name: field,
        type: typeOf(ps, `${unique}${pascal(field)}`),
        required: req.has(field),
        nullable: nullable(ps),
        description: describe(ps),
      })
    }
    return { kind: 'ref', name: unique }
  }
  function declare(name) {
    if (types.has(name)) return
    const s = schemas[name]
    if (!s) throw new Error(`schema ${name} is missing`)
    if (s.properties || s.allOf) {
      objectType(s.allOf ? { ...s, properties: mergedProps(s), required: mergedRequired(s) } : s, name)
      return
    }
    types.set(name, { kind: 'alias', description: s.description ?? '', type: null, source: s })
    types.get(name).type = typeOf({ ...s, description: undefined }, name)
  }

  const methodName = (op, ns) => {
    if (names.overrides[op.operationId]) return names.overrides[op.operationId]
    let n = op.operationId.replace(/^get/, '')
    for (const noun of [...(names.namespaces[ns] ?? [])].sort((a, b) => b.length - a.length)) {
      if (n.startsWith(noun)) {
        n = n.slice(noun.length)
        break
      }
    }
    return n ? n[0].toLowerCase() + n.slice(1) : 'get'
  }

  const operations = []
  for (const [path, item] of Object.entries(doc.paths)) {
    for (const [method, op] of Object.entries(item)) {
      if (!op?.operationId) continue
      const tag = op.tags?.[0]
      const namespace = tag === 'resolve' ? null : tag
      if (namespace && !(namespace in names.namespaces)) throw new Error(`${op.operationId}: tag ${tag} has no namespace in names.json`)
      const params = (op.parameters ?? []).filter((p) => p.name !== 'chain_id')
      const ok = op.responses?.['200']?.content?.['application/json']?.schema
      operations.push({
        id: op.operationId,
        namespace,
        name: methodName(op, namespace),
        method: method.toUpperCase(),
        path: path.replace('/chains/{chain_id}', '/chains/{chainId}'),
        summary: op.summary ?? '',
        description: op.description ?? '',
        docsUrl: op.externalDocs?.url ?? '',
        plan: op['x-minimum-plan'] ?? '',
        stability: stability[op.operationId] ?? (() => { throw new Error(`${op.operationId}: no stability — run sync`) })(),
        pathParams: params.filter((p) => p.in === 'path').map((p) => ({ name: p.name, type: typeOf(p.schema, 'Param'), description: p.description ?? '' })),
        queryParams: params
          .filter((p) => p.in === 'query')
          .map((p) => ({ name: p.name, type: typeOf(p.schema, 'Param'), required: !!p.required, description: p.description ?? '' })),
        paged: params.some((p) => p.in === 'query' && p.name === 'cursor'),
        response: ok ? typeOf(ok, `${pascal(op.operationId.replace(/^get/, ''))}Response`) : { kind: 'any' },
      })
    }
  }
  // Every namespace + name is unique.
  const seen = new Set()
  for (const o of operations) {
    const k = `${o.namespace}.${o.name}`
    if (seen.has(k)) throw new Error(`two operations are named ${k}; add an override to names.json`)
    seen.add(k)
  }
  for (const t of types.values()) delete t.source
  return { version: doc.info.version, operations, types }
}
