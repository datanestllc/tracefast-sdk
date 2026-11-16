// Generate every SDK from the committed catalogue snapshot — no network.
//
//   node codegen/generate.mjs           # write the generated files
//   node codegen/generate.mjs --check   # exit 1 if a generated file is out of date
import { readFileSync, writeFileSync, existsSync, mkdirSync } from 'node:fs'
import { dirname } from 'node:path'
import { fileURLToPath } from 'node:url'
import { loadIr } from './ir.mjs'
import { emitTs } from './emit-ts.mjs'

const emitters = [emitTs]
for (const name of ['emit-python.mjs', 'emit-rust.mjs']) {
  if (existsSync(new URL(name, import.meta.url))) emitters.push((await import(`./${name}`)).default)
}

const ir = loadIr()
const root = new URL('../', import.meta.url)
const check = process.argv.includes('--check')
let stale = 0
for (const emit of emitters) {
  for (const [rel, text] of Object.entries(emit(ir))) {
    const file = fileURLToPath(new URL(rel, root))
    const before = existsSync(file) ? readFileSync(file, 'utf8') : null
    if (before === text) continue
    stale++
    if (check) console.log(`out of date: ${rel}`)
    else {
      mkdirSync(dirname(file), { recursive: true })
      writeFileSync(file, text)
    }
  }
}
console.log(`${ir.operations.length} operations, ${ir.types.size} types, catalogue ${ir.version}: ${stale} file(s) ${check ? 'out of date' : 'written'}`)
process.exit(check && stale ? 1 : 0)
