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
  const LABELS = { string: 'Text', boolean: 'Bool', integer: 'Integer', number: 'Number', enum: 'Text', array: 'List', map: 'Record' }

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
      // A union: one variant per alternative, labelled by what it is. Scalars
      // first, then lists, then objects, so a decoder that tries them in order
      // cannot read a list as an object.
      const rank = { string: 0, boolean: 0, integer: 0, number: 0, enum: 0, array: 1 }
      const variants = alts
        .map((a) => {
          const type = typeOf(a, `${hint}Variant`)
          const label = type.kind === 'ref' ? type.name : LABELS[type.kind] ?? pascal(type.kind)
          return { label, type, description: clean(a.description ?? '') }
        })
        .sort((a, b) => (rank[a.type.kind] ?? 2) - (rank[b.type.kind] ?? 2))
      const labels = variants.map((v) => v.label)
      if (new Set(labels).size !== labels.length) throw new Error(`${hint}: two union variants share the label ${labels}`)
      return { kind: 'union', variants }
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
  // Catalogue copy written for listings, not for a hover: the product prefix, and
  // Rust range syntax in parameter notes.
  const clean = (t) =>
    (t ?? '')
      .replace(/^TraceFast (live Ethereum data|knowledge graph):\s*/, '')
      .replace(/(\d+)\.\.=(\d+)/g, '$1 to $2')
      .replace(/^[a-z]/, (c) => c.toUpperCase())
      // The catalogue hard-wraps its prose; a single line break is a space, a
      // blank line or a list item keeps its break.
      .replace(/([^\n])\n(?!\n|\s*(?:[-*]|\d+\.)\s)/g, '$1 ')
  const describe = (p) => p?.description ?? (p?.allOf ?? p?.oneOf ?? []).map((a) => a.description).find(Boolean) ?? ''

  const objectType = (s, name) => {
    if (!name) throw new Error('inline object without a name')
    // A public type name never changes silently: two schemas wanting one name fail generation.
    const unique = name
    if (types.has(unique)) {
      if (types.get(unique).source === s) return { kind: 'ref', name: unique }
      throw new Error(`two schemas want the type name ${unique}; rename one in the catalogue or the generator`)
    }
    const entry = { kind: 'object', description: clean(s.description), fields: [], source: s }
    types.set(unique, entry)
    // Keys beyond the listed ones, where the catalogue allows them.
    if (s.additionalProperties !== undefined && s.additionalProperties !== false)
      entry.extra = { type: s.additionalProperties === true ? { kind: 'any' } : typeOf(s.additionalProperties, `${unique}Extra`), description: clean(s.additionalProperties.description ?? '') }
    const req = new Set(s.required ?? [])
    for (const [field, ps] of Object.entries(s.properties ?? {})) {
      entry.fields.push({
        name: field,
        type: typeOf(ps, `${unique}${pascal(field)}`),
        required: req.has(field),
        nullable: nullable(ps),
        description: clean(describe(ps)),
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
    types.set(name, { kind: 'alias', description: clean(s.description), type: null, source: s })
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

  const sentence = (t) => (t && !/[.!?]$/.test(t) ? `${t}.` : t)
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
        summary: sentence(op.summary ?? ''),
        description: clean(op.description),
        docsUrl: op.externalDocs?.url ?? '',
        plan: op['x-minimum-plan'] ?? '',
        stability: stability[op.operationId] ?? (() => { throw new Error(`${op.operationId}: no stability — run sync`) })(),
        pathParams: params.filter((p) => p.in === 'path').map((p) => ({ name: p.name, type: typeOf(p.schema, 'Param'), description: clean(p.description) })),
        queryParams: params
          .filter((p) => p.in === 'query')
          .map((p) => ({ name: p.name, type: typeOf(p.schema, 'Param'), required: !!p.required, description: clean(p.description) })),
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
  // The row array each paged operation's item iterator walks.
  for (const o of operations.filter((x) => x.paged)) {
    const env = types.get(o.response.name)
    const data = types.get(env.fields.find((f) => f.name === 'data').type.name)
    const arrays = data.fields.filter((f) => f.type.kind === 'array').map((f) => f.name)
    const chosen = names.items?.[o.id] ?? (arrays.length === 1 ? arrays[0] : undefined)
    if (!chosen || !arrays.includes(chosen)) throw new Error(`${o.id}: cannot tell its row array among [${arrays}]; add it to names.json items`)
    o.itemsField = chosen
    o.itemType = data.fields.find((f) => f.name === chosen).type.items
  }
  // A schema the generator cannot type fails generation, unless it is a known gap.
  const allowed = new Set(names.untyped?.allow ?? [])
  for (const [n, t] of types) {
    if (t.kind === 'alias' && t.type.kind === 'any' && !allowed.has(n)) throw new Error(`${n} has no type in the catalogue`)
    if (t.kind !== 'object') continue
    for (const f of t.fields) if (f.type.kind === 'any' && !allowed.has(`${n}.${f.name}`)) throw new Error(`${n}.${f.name} has no type in the catalogue`)
  }
  for (const t of types.values()) delete t.source
  return { version: doc.info.version, operations, types }
}
