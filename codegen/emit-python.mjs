// Python: python/src/tracefast/_generated/{__init__,types,api}.py from the IR.
//
// Responses are TypedDicts (plain dicts at runtime: an absent key stays absent,
// null stays None). Every namespace is emitted twice from one template — a sync
// class over `SyncHttp` and an async one over `AsyncHttp` — so both clients are
// fully typed under `mypy --strict`.
const PLAN = { data: 'Enriched Data', intelligence: 'Intelligence', investigation: 'Investigation' }
const WIDTH = 88

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
export const pyType = (t) => {
  switch (t.kind) {
    case 'ref':
      return t.name
    case 'string':
      return 'str'
    case 'integer':
      return 'int'
    case 'number':
      return 'float'
    case 'boolean':
      return 'bool'
    case 'array':
      return `list[${pyType(t.items)}]`
    case 'map':
      return `dict[str, ${pyType(t.values)}]`
    case 'enum':
      return `Literal[${t.values.map(str).join(', ')}] | str`
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
  for (const [name, t] of sorted) {
    exported.push(name)
    out += '\n\n'
    if (t.kind === 'alias') {
      out += comment(t.description, '')
      out += `${name}: TypeAlias = ${str(pyType(t.type))}\n`
      continue
    }
    const functional = t.fields.some((f) => !isIdent(f.name) || SHADOWED.has(f.name) || typeNames.has(f.name))
    if (functional) {
      // A field named like a keyword (`from`) needs the functional syntax; the
      // annotations are evaluated at runtime here, so each one is a string.
      out += comment(t.description, '')
      out += `${name} = TypedDict(\n    ${str(name)},\n    {\n`
      for (const f of t.fields) {
        out += comment(f.description, '        ')
        const ft = fieldType(f)
        const m = ft.match(/^NotRequired\[(.*)\]$/)
        out += `        ${str(f.name)}: ${m ? `NotRequired[${str(m[1])}]` : str(ft)},\n`
      }
      out += '    },\n)\n'
      continue
    }
    out += `class ${name}(TypedDict):\n`
    const doc = docstring([t.description], '    ')
    out += doc
    if (!t.fields.length) out += doc ? '' : '    pass\n'
    t.fields.forEach((f, i) => {
      if (i > 0 || doc) out += '\n'
      out += `    ${f.name}: ${fieldType(f)}\n`
      out += docstring([f.description], '    ')
    })
  }
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
  const ann = (t) => {
    const s = pyType(t)
    if (/\bAny\b/.test(s)) usesAny = true
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
    const res = ann(op.response)
    // Required query parameters first: they take no default.
    const ordered = [...query.filter((q) => q.required), ...query.filter((q) => !q.required)]
    const sig = [
      'self',
      ...path.map((p) => `${p.py}: ${p.type.kind === 'integer' ? 'int | str' : 'str'}`),
      ...(ordered.length ? ['*'] : []),
      ...ordered.map((q) => (q.required ? `${q.py}: ${ann(q.type)}` : `${q.py}: ${ann(q.type)} | None = None`)),
    ]
    const pathExpr =
      'f' +
      str(
        op.path
          .replace(/\{(\w+)\}/g, (all, n) => (n === 'chainId' ? all : `{segment(${pyName(n)})}`))
          .replace('{chainId}', '{self._http.chain_id}'),
      )
    const queryExpr = query.length ? `{${query.map((q) => `${str(q.name)}: ${q.py}`).join(', ')}}` : 'None'
    const args = [
      ...path.map((p) => `    ${p.py}: ${oneLine(p.description) || 'Path parameter.'}`),
      ...query.map((q) => `    ${q.py}: ${oneLine(q.description) || 'Query parameter.'}${q.py !== q.name ? ` Sent as \`\`${q.name}\`\`.` : ''}${q.required ? '' : ' Left as None, it is not sent.'}`),
    ]
    const tail = (extra) => [
      `Plan: ${PLAN[op.plan] ?? op.plan} or higher.${op.stability === 'beta' ? ' **Beta** — the shape may still change.' : ''}`,
      ...(args.length ? ['', 'Args:', ...args] : []),
      '',
      ...extra,
      `Docs: ${op.docsUrl}`,
    ]
    const doc = docstring([op.summary, '', op.description, '', ...tail([])], '        ')
    const sigText = (name, ret) => {
      const one = `    ${async_ ? 'async ' : ''}def ${name}(${sig.join(', ')}) -> ${ret}:`
      if (one.length <= WIDTH) return one + '\n'
      return `    ${async_ ? 'async ' : ''}def ${name}(\n${sig.map((s) => `        ${s},`).join('\n')}\n    ) -> ${ret}:\n`
    }
    let m = sigText(pyMethod(op), res) + doc
    const call = `self._http.get(${pathExpr}, ${queryExpr})`
    m += `        return cast(${str(res)}, ${async_ ? 'await ' : ''}${call})\n`
    if (op.paged) {
      const it = `${async_ ? 'AsyncIterator' : 'Iterator'}[${res}]`
      m +=
        '\n' +
        sigText(`${pyMethod(op)}_pages`, it).replace('async def', 'def') +
        docstring(
          [
            `Every page of \`${label}\`, following \`data.nextCursor\` to the end.`,
            '',
            `${async_ ? 'An async iterator' : 'A generator'} of whole responses; the cursor goes back unchanged with the same other parameters. The pager never stops on a short page.`,
            '',
            ...tail([]),
          ],
          '        ',
        ) +
        `        return self._http.pages(${pathExpr}, ${queryExpr})\n`
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
        docstring([`\`client.${ns}\` — the ${ns} methods of the TraceFast API${async_ ? ' (async)' : ''}.`], '    ') +
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
      docstring([`The generated part of \`${async_ ? 'AsyncTraceFast' : 'TraceFast'}\`: its namespaces and the methods on the client itself.`], '    ') +
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
    `from collections.abc import AsyncIterator, Iterator\nfrom typing import ${usesAny ? 'Any, ' : ''}cast\n\n` +
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
