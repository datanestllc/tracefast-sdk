// Words, hosts and secrets that must never be in this repository — generated
// code and fixtures included. Exit 1 on a hit.
//
//   node codegen/lint.mjs
import { readdirSync, readFileSync, statSync } from 'node:fs'
import { join, relative } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = fileURLToPath(new URL('../', import.meta.url))
const SKIP = new Set(['.git', 'node_modules', 'dist', 'target', '.venv', '__pycache__', '.pytest_cache', '.mypy_cache', 'Cargo.lock', 'package-lock.json', 'uv.lock'])
const RULES = [
  [/\bAtlas\b/, 'internal name'],
  [/DeepTrace/i, 'old brand'],
  // A loopback host on its own is allowed (the https rule exempts it); with a port or a
  // service path it is somebody's machine.
  [/(127\.0\.0\.1|localhost):\d|:400[13]\b|\/dev\/public|\/api-docs\//, 'internal host'],
  [/sepolia|eip155:84532/i, 'test network'],
  [/\bdt_[a-z0-9]/, 'old key prefix'],
  [/tf_(live|test)_[A-Za-z0-9]{16,}/, 'what looks like a real key'],
  [/\bcredits?\b|\bwaitlist\b/i, 'word the API does not have'],
  [/[\u0400-\u04FF]/, 'not English — code, comments and docs are English only'],
]
const ALLOW = new Set(['codegen/lint.mjs'])
let hits = 0
const walk = (dir) => {
  for (const name of readdirSync(dir)) {
    if (SKIP.has(name)) continue
    const p = join(dir, name)
    if (statSync(p).isDirectory()) walk(p)
    else {
      const rel = relative(root, p)
      if (ALLOW.has(rel) || !/\.(m?[jt]s|json|md|py|rs|toml|ya?ml|txt|cfg)$|^LICENSE$/.test(name)) continue
      const lines = readFileSync(p, 'utf8').split('\n')
      lines.forEach((line, i) => {
        for (const [re, why] of RULES) if (re.test(line)) (hits++, console.log(`${rel}:${i + 1}: ${why}: ${line.trim().slice(0, 120)}`))
      })
    }
  }
}
walk(root)
console.log(hits ? `${hits} hit(s)` : 'clean')
process.exit(hits ? 1 : 0)
