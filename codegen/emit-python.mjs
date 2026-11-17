// Python: python/src/tracefast/_generated/{__init__,types,api}.py from the IR.
//
// Responses are TypedDicts (plain dicts at runtime: an absent key stays absent,
// null stays None). Every namespace is emitted twice from one template — a sync
// class over `SyncHttp` and an async one over `AsyncHttp` — so both clients are
// fully typed under `mypy --strict`.
const PLAN = { data: 'Enriched Data', intelligence: 'Intelligence', investigation: 'Investigation' }
const WIDTH = 88
const NOT_SENT = 'A keyword argument left as None is not sent: the API applies its own default.'

const KEYWORDS = new Set([
  'False', 'None', 'True', 'and', 'as', 'assert', 'async', 'await', 'break', 'class', 'continue', 'def', 'del', 'elif',
  'else', 'except', 'finally', 'for', 'from', 'global', 'if', 'import', 'in', 'is', 'lambda', 'nonlocal', 'not', 'or',
  'pass', 'raise', 'return', 'try', 'while', 'with', 'yield',
])
// Names a TypedDict field may not take in class syntax: a keyword, or a name the
// annotations of the module use (the field would shadow it in the class body).
const SHADOWED = new Set(['str', 'int', 'float', 'bool', 'list', 'dict', 'Any', 'Literal', 'NotRequired', 'TypedDict', 'TypeAlias', 'None'])

/** `fromBlock` → `from_block`; a keyword gets a trailing underscore (`from` → `from_`). */
export const pyName = (wire) => {
  const s = wire.replace(/([a-z0-9])([A-Z])/g, '$1_$2').replace(/([A-Z])([A-Z][a-z])/g, '$1_$2').toLowerCase()
  return KEYWORDS.has(s) ? `${s}_` : s
}
/** The Python method name of an operation (`attributesAtBlock` → `attributes_at_block`). */
export const pyMethod = (op) => pyName(op.name)

const pascal = (s) => s[0].toUpperCase() + s.slice(1)
const isIdent = (n) => /^[A-Za-z_][A-Za-z0-9_]*$/.test(n) && !KEYWORDS.has(n)
const str = (s) => JSON.stringify(s) // a JSON string is a valid Python string literal

/** A type reference → its Python annotation text. */
// `self`: the alias being written. A recursive alias names itself in quotes —
// a forward reference that mypy and typing.get_type_hints both resolve.
export const pyType = (t, self) => {
  switch (t.kind) {
    case 'ref':
      return t.name === self ? JSON.stringify(t.name) : t.name
    case 'string':
      return 'str'
    case 'integer':
      return 'int'
    case 'number':
      return 'float'
    case 'boolean':
      return 'bool'
    case 'array':
      return `list[${pyType(t.items, self)}]`
    case 'map':
      return `dict[str, ${pyType(t.values, self)}]`
    case 'enum':
      return `Literal[${t.values.map(str).join(', ')}] | str`
    case 'union':
      return t.variants.map((v) => pyType(v.type, self)).join(' | ')
    default:
      return 'Any'
  }
}

// ---- text helpers

const wrap = (text, width) => {
  const out = []
  for (const para of text.split('\n')) {
    if (!para.trim()) {
      out.push('')
      continue
    }
    const lead = para.match(/^\s*/)[0]
    const start = out.length
    let line = ''
    for (const word of para.trim().split(/\s+/)) {
      const ind = out.length > start && lead ? `${lead}    ` : lead
      if (line && ind.length + line.length + 1 + word.length > width) {
        out.push(ind + line)
        line = word
      } else line = line ? `${line} ${word}` : word
    }
    if (line) out.push((out.length > start && lead ? `${lead}    ` : lead) + line)
  }
  return out
}
const escapeDoc = (s) => s.replace(/\\/g, '\\\\').replace(/"""/g, '\\"\\"\\"')
/** A docstring at `indent`, closing quotes on their own line. */
const docstring = (lines, indent) => {
  const text = lines.filter((l) => l !== undefined && l !== null).join('\n').replace(/\n{3,}/g, '\n\n').trim()
  if (!text) return ''
  const body = wrap(escapeDoc(text), WIDTH - indent.length)
  if (body.length === 1 && !body[0].endsWith('"') && indent.length + body[0].length + 6 <= WIDTH) return `${indent}"""${body[0]}"""\n`
  return `${indent}"""${body[0]}\n${body.slice(1).map((l) => (l ? indent + l : '')).join('\n')}\n${indent}"""\n`
}
const comment = (text, indent) =>
  text.trim() ? wrap(text.trim(), WIDTH - indent.length - 2).map((l) => (l ? `${indent}# ${l}` : `${indent}#`)).join('\n') + '\n' : ''
const oneLine = (s) => s.replace(/\s+/g, ' ').trim()
const capital = (s) => (s ? s[0].toUpperCase() + s.slice(1) : s)
const sentence = (s) => (s && !/[.!?:]$/.test(s) ? `${s}.` : s)

// ---- types.py

function emitTypes(ir, header) {
  const sorted = [...ir.types].sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0))
  const typeNames = new Set(ir.types.keys())
  const fieldType = (f) => {
    let t = pyType(f.type)
    if (f.nullable) t += ' | None'
    return f.required ? t : `NotRequired[${t}]`
  }
  let out =
    header +
    docstring(['The response types of the TraceFast API, one TypedDict per catalogue object.', '', 'A key the API leaves out is absent from the dict; a `null` is `None`. Enums are open: a value the catalogue does not list may appear.'], '') +
    '\nfrom __future__ import annotations\n\nfrom typing import Any, Literal, TypeAlias\n\nfrom typing_extensions import NotRequired, TypedDict\n'
  const exported = []
  const hasRef = (t) =>
    t.kind === 'ref' || (t.items && hasRef(t.items)) || (t.values && hasRef(t.values)) || (t.variants ?? []).some((v) => hasRef(v.type))
  const refsOf = (t) => (t.kind === 'ref' ? [t.name] : [t.items, t.values, ...(t.variants ?? []).map((v) => v.type)].filter(Boolean).flatMap(refsOf))
  // An alias is evaluated at runtime (typing.get_type_hints reads it), so one
  // that names a class is written after every class.
  // Aliases that name another type go after every class, in dependency order:
  // an alias naming another such alias follows it; one naming itself quotes it.
  const deferred = new Map()
  for (const [name, t] of sorted) {
    exported.push(name)
    if (t.kind === 'alias') {
      const text = '\n\n' + comment(t.description, '') + `${name}: TypeAlias = ${pyType(t.type, name)}\n`
      if (!hasRef(t.type)) {
        out += text
        continue
      }
      const after = new Set()
      for (const r of refsOf(t.type)) {
        if (r === name) continue
        const k = ir.types.get(r)
        if (k.kind === 'alias' && hasRef(k.type)) after.add(r)
      }
      deferred.set(name, { text, after })
      continue
    }
    out += '\n\n'
    // A field named like a keyword (`from`) cannot be written in class syntax:
    // those fields go into a functional base, the others keep their docstrings.
    const special = (f) => !isIdent(f.name) || SHADOWED.has(f.name) || typeNames.has(f.name)
    const kw = t.fields.filter(special)
    const plain = t.fields.filter((f) => !special(f))
    let base = 'TypedDict'
    if (kw.length) {
      base = `_${name}Kw`
      out += `${base} = TypedDict(\n    ${str(base)},\n    {\n`
      for (const f of kw) {
        // The functional form is evaluated at runtime: only builtins may appear.
        if (hasRef(f.type)) throw new Error(`${name}.${f.name}: a keyword field may not refer to another type`)
        out += comment(f.description, '        ')
        out += `        ${str(f.name)}: ${fieldType(f)},\n`
      }
      out += '    },\n)\n\n\n'
    }
    out += `class ${name}(${base}):\n`
    const kwDocs = kw.filter((f) => f.description).map((f) => `\`${f.name}\`: ${oneLine(f.description)}`)
    const doc = docstring([t.description, ...(kwDocs.length ? ['', 'Fields named like a Python keyword:', '', ...kwDocs.map((d) => `- ${d}`)] : [])], '    ')
    out += doc
    if (!plain.length) out += doc ? '' : '    pass\n'
    plain.forEach((f, i) => {
      if (i > 0 || doc) out += '\n'
      out += `    ${f.name}: ${fieldType(f)}\n`
      out += docstring([f.description], '    ')
    })
  }
  const written = new Set()
  const write = (name, path = []) => {
    if (written.has(name)) return
    if (path.includes(name)) throw new Error(`aliases ${[...path, name].join(' → ')} refer to each other; the Python emitter cannot order them`)
    const d = deferred.get(name)
    for (const r of [...d.after].sort()) write(r, [...path, name])
    written.add(name)
    out += d.text
  }
  for (const name of deferred.keys()) write(name)
  out += `\n\n__all__ = [\n${exported.map((n) => `    ${str(n)},`).join('\n')}\n]\n`
  return out
}

// ---- api.py

function emitApi(ir, header) {
  const groups = new Map()
  for (const op of ir.operations) {
    const ns = op.namespace ?? 'root'
    if (!groups.has(ns)) groups.set(ns, [])
    groups.get(ns).push(op)
  }
  const used = new Set()
  let usesAny = false
  let usesLiteral = false
  const ann = (t) => {
    const s = pyType(t)
    if (/\bAny\b/.test(s)) usesAny = true
    if (/\bLiteral\[/.test(s)) usesLiteral = true
    for (const m of s.matchAll(/\b[A-Z]\w*/g)) if (ir.types.has(m[0])) used.add(m[0])
    return s
  }

  const method = (op, async_) => {
    const label = `${op.namespace ? `${op.namespace}.` : ''}${pyMethod(op)}`
    const seen = new Set()
    const own = (n, wire) => {
      if (seen.has(n)) throw new Error(`${op.id}: two parameters are named ${n} in Python (${wire})`)
      seen.add(n)
      return n
    }
    const path = op.pathParams.map((p) => ({ ...p, py: own(pyName(p.name), p.name) }))
    const query = op.queryParams.map((q) => ({ ...q, py: own(pyName(q.name), q.name) }))
    for (const extra of ['max_pages', 'max_items']) if (op.paged && seen.has(extra)) throw new Error(`${op.id}: a parameter is named ${extra}`)
    const res = ann(op.response)
    // Required query parameters first: they take no default.
    const ordered = [...query.filter((q) => q.required), ...query.filter((q) => !q.required)]
    const sig = (extra) => [
      'self',
      ...path.map((p) => `${p.py}: ${p.type.kind === 'integer' ? 'int' : 'str'}`),
      ...(ordered.length || extra.length ? ['*'] : []),
      ...ordered.map((q) => (q.required ? `${q.py}: ${ann(q.type)}` : `${q.py}: ${ann(q.type)} | None = None`)),
      ...extra,
    ]
    const pathExpr =
      'f' +
      str(
        op.path
          .replace(/\{(\w+)\}/g, (all, n) => (n === 'chainId' ? all : `{segment(${pyName(n)})}`))
          .replace('{chainId}', '{self._http.chain_id}'),
      )
    const queryExpr = query.length ? `{${query.map((q) => `${str(q.name)}: ${q.py}`).join(', ')}}` : 'None'
    const args = (extra) => [
      ...path.map((p) => `    ${p.py}: ${sentence(oneLine(p.description)) || 'Path parameter.'}`),
      ...query.map((q) => `    ${q.py}: ${sentence(oneLine(q.description)) || 'Query parameter.'}${q.py !== q.name ? ` Sent as \`${q.name}\`.` : ''}`),
      ...extra,
    ]
    const plan = `Plan: ${PLAN[op.plan] ?? op.plan} or higher.${op.stability === 'beta' ? ' **Beta**: the shape may still change.' : ''}`
    const raises = (iterating) => [
      'Raises:',
      `    TraceFastError: ${iterating ? 'While iterating: the' : 'The'} API refused the call or it failed; the subclass and \`code\` say why.`,
      ...(path.length ? ['    ValueError: A path value is empty, `.` or `..`; nothing is sent.'] : []),
    ]
    const doc = (lead, extraArgs, returns, iterating = false) =>
      docstring(
        [
          ...lead,
          '',
          plan,
          '',
          'Args:',
          ...args(extraArgs),
          '',
          'Returns:',
          `    ${returns}`,
          '',
          ...raises(iterating),
          '',
          `Docs: ${op.docsUrl}`,
        ].filter((l, i, all) => !(l === 'Args:' && all[i + 1] === '')),
        '        ',
      )
    const sigText = (name, ret, extra) => {
      const parts = sig(extra)
      const one = `    ${async_ ? 'async ' : ''}def ${name}(${parts.join(', ')}) -> ${ret}:`
      if (one.length <= WIDTH) return one + '\n'
      return `    ${async_ ? 'async ' : ''}def ${name}(\n${parts.map((s) => `        ${s},`).join('\n')}\n    ) -> ${ret}:\n`
    }
    let m =
      sigText(pyMethod(op), res, []) +
      doc([op.summary, '', capital(op.description)], [], `\`${res}\`: the whole answer, \`data\` and \`meta\`.`) +
      `        return cast(${str(res)}, ${async_ ? 'await ' : ''}self._http.get(${pathExpr}, ${queryExpr}))\n`
    if (op.paged) {
      const walker = async_ ? 'An async iterator' : 'An iterator'
      const pagesT = `${async_ ? 'AsyncIterator' : 'Iterator'}[${res}]`
      const item = ann(op.itemType)
      const itemsT = `${async_ ? 'AsyncIterator' : 'Iterator'}[${item}]`
      const stop = 'It stops when `data.nextCursor` is absent, never on a short page, and passes the cursor back unchanged with the same other parameters. A `cursor` given here is the first one sent.'
      m +=
        '\n' +
        sigText(`${pyMethod(op)}_pages`, pagesT, ['max_pages: int | None = None']).replace('async def', 'def') +
        doc(
          [`Every page of \`${label}\`, one whole response at a time.`, '', stop],
          ['    max_pages: Stop after this many pages. Left as None, the walk goes to the last page.'],
          `${walker} of \`${res}\`, \`meta\` included.`,
          true,
        ) +
        `        return self._http.pages(${pathExpr}, ${queryExpr}, max_pages)\n` +
        '\n' +
        sigText(`${pyMethod(op)}_items`, itemsT, ['max_items: int | None = None']).replace('async def', 'def') +
        doc(
          [`Every row of \`data.${op.itemsField}\` of \`${label}\`, page after page.`, '', stop],
          ['    max_items: Stop after this many rows; no further page is fetched. Left as None, the walk goes to the last page.'],
          `${walker} of \`${item}\`.`,
          true,
        ) +
        `        return self._http.items(${pathExpr}, ${queryExpr}, ${str(op.itemsField)}, max_items)\n`
    }
    return m
  }

  const nsClass = (ns, async_) => `${async_ ? 'Async' : ''}${pascal(ns)}Api`
  let classes = ''
  for (const async_ of [false, true]) {
    const http = async_ ? 'AsyncHttp' : 'SyncHttp'
    for (const [ns, ops] of groups) {
      if (ns === 'root') continue
      classes +=
        `\n\nclass ${nsClass(ns, async_)}:\n` +
        docstring([`\`client.${ns}\`: the ${ns} methods of the TraceFast API${async_ ? ', as coroutines' : ''}.`, '', NOT_SENT], '    ') +
        `\n    def __init__(self, http: ${http}) -> None:\n        self._http = http\n\n` +
        ops.map((op) => method(op, async_)).join('\n')
    }
  }
  // The base of each client: its namespaces and the operations that live on it.
  for (const async_ of [false, true]) {
    const http = async_ ? 'AsyncHttp' : 'SyncHttp'
    const nss = [...groups.keys()].filter((n) => n !== 'root')
    classes +=
      `\n\nclass ${async_ ? 'AsyncClientBase' : 'ClientBase'}:\n` +
      docstring([`The generated part of \`${async_ ? 'AsyncTraceFast' : 'TraceFast'}\`: its namespaces and the methods on the client itself.`, '', NOT_SENT], '    ') +
      '\n' +
      nss.map((ns) => `    ${ns}: ${nsClass(ns, async_)}\n`).join('') +
      `\n    def __init__(self, http: ${http}) -> None:\n        self._http = http\n` +
      nss.map((ns) => `        self.${ns} = ${nsClass(ns, async_)}(http)\n`).join('') +
      ((groups.get('root') ?? []).length ? '\n' + groups.get('root').map((op) => method(op, async_)).join('\n') : '')
  }

  const nss = [...groups.keys()].filter((n) => n !== 'root')
  const exported = [...nss.flatMap((ns) => [nsClass(ns, false), nsClass(ns, true)]), 'ClientBase', 'AsyncClientBase', 'NAMESPACES']
  return (
    header +
    docstring(['The operations of the TraceFast API, one method each, grouped by namespace.'], '') +
    '\nfrom __future__ import annotations\n\n' +
    `from collections.abc import AsyncIterator, Iterator\nfrom typing import ${usesAny ? 'Any, ' : ''}${usesLiteral ? 'Literal, ' : ''}cast\n\n` +
    'from .._http import AsyncHttp, SyncHttp, segment\n' +
    `from .types import (\n${[...used].sort().map((t) => `    ${t},`).join('\n')}\n)\n\n` +
    '# The namespaces of the client, in catalogue order.\n' +
    `NAMESPACES: tuple[str, ...] = (${nss.map(str).join(', ')},)\n` +
    classes +
    `\n\n__all__ = [\n${exported.map((n) => `    ${str(n)},`).join('\n')}\n]\n`
  )
}

export default function emitPython(ir) {
  const header = `# Generated by codegen/ from the TraceFast API catalogue ${ir.version}. Do not edit.\n`
  return {
    'python/src/tracefast/_generated/__init__.py':
      header + docstring(['Code generated from the TraceFast API catalogue. Do not edit; run `node codegen/generate.mjs`.'], '') + `\nCATALOGUE_VERSION = ${str(ir.version)}\n`,
    'python/src/tracefast/_generated/types.py': emitTypes(ir, header),
    'python/src/tracefast/_generated/api.py': emitApi(ir, header),
  }
}
