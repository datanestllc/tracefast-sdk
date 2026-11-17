import { readFileSync } from 'node:fs'
// The language-neutral model the SDK was generated from: the tests check the
// fixtures against it strictly, so a gap between catalogue and API shows up.
// @ts-expect-error — plain ESM module without types
import { loadIr } from '../../codegen/ir.mjs'

export const ir = loadIr()
export const samples = JSON.parse(readFileSync(new URL('../../fixtures/samples.json', import.meta.url), 'utf8'))
export const fixture = (id: string) => JSON.parse(readFileSync(new URL(`../../fixtures/${id}.json`, import.meta.url), 'utf8'))

type T = { kind: string; name?: string; items?: T; values?: T; values_?: string[] }

/** Strict walk of a value against an IR type: required fields, nullability, types, unknown fields. */
export function validate(value: unknown, type: any, path: string, out: string[]): void {
  switch (type.kind) {
    case 'ref': {
      const t = ir.types.get(type.name)
      if (t.kind === 'alias') return validate(value, t.type, path, out)
      if (!value || typeof value !== 'object' || Array.isArray(value)) return void out.push(`${path}: expected object ${type.name}`)
      const obj = value as Record<string, unknown>
      const known = new Set<string>()
      for (const f of t.fields) {
        known.add(f.name)
        if (!(f.name in obj)) {
          if (f.required) out.push(`${path}.${f.name}: required, absent`)
          continue
        }
        if (obj[f.name] === null) {
          if (!f.nullable) out.push(`${path}.${f.name}: null, not nullable`)
          continue
        }
        validate(obj[f.name], f.type, `${path}.${f.name}`, out)
      }
      for (const k of Object.keys(obj)) {
        if (known.has(k)) continue
        if (t.extra) validate(obj[k], t.extra.type, `${path}.${k}`, out)
        else out.push(`${path}.${k}: not in the catalogue`)
      }
      return
    }
    case 'union': {
      // One variant must take the value whole.
      const tries = type.variants.map((v: any) => {
        const errs: string[] = []
        validate(value, v.type, path, errs)
        return errs
      })
      if (!tries.some((e: string[]) => e.length === 0)) out.push(`${path}: matches no variant (${tries.map((e: string[]) => e[0]).join('; ')})`)
      return
    }
    case 'array':
      if (!Array.isArray(value)) return void out.push(`${path}: expected array`)
      value.forEach((v, i) => validate(v, type.items, `${path}[${i}]`, out))
      return
    case 'map':
      if (!value || typeof value !== 'object') return void out.push(`${path}: expected map`)
      for (const [k, v] of Object.entries(value)) validate(v, type.values, `${path}.${k}`, out)
      return
    case 'string':
    case 'enum':
      if (typeof value !== 'string') out.push(`${path}: expected string, got ${typeof value}`)
      return
    case 'integer':
      if (!Number.isInteger(value)) out.push(`${path}: expected integer`)
      return
    case 'number':
      if (typeof value !== 'number') out.push(`${path}: expected number`)
      return
    case 'boolean':
      if (typeof value !== 'boolean') out.push(`${path}: expected boolean`)
      return
  }
}

/** A fetch that answers from a list, recording every request. */
export function mockFetch(answers: Array<() => Response>) {
  const calls: Array<{ url: string; headers: Record<string, string> }> = []
  const fn = async (url: string | URL | Request, init?: RequestInit) => {
    calls.push({ url: String(url), headers: Object.fromEntries(Object.entries(init?.headers ?? {})) })
    const next = answers.shift()
    if (!next) throw new Error('no more answers')
    return next()
  }
  return { fetch: fn as typeof globalThis.fetch, calls }
}

export const json = (body: unknown, status = 200, headers: Record<string, string> = {}) => () =>
  new Response(JSON.stringify(body), { status, headers: { 'content-type': 'application/json', ...headers } })
