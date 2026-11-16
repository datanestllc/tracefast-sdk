import { makeError, type TraceFastError } from './errors.ts'
import { VERSION } from './version.ts'

/** Per-call options — the last argument of every method. */
export interface RequestOptions {
  /** Cancels the call; it then rejects with `RequestAbortedError`. */
  signal?: AbortSignal
  /** Overrides the client's timeout for this call, in milliseconds per attempt. */
  timeout?: number
  /** Overrides the client's `maxRetries` for this call. */
  maxRetries?: number
}

/** Options of a `…Pages()` walker. */
export interface PagesOptions extends RequestOptions {
  /** Stop after this many pages. */
  maxPages?: number
}

/** Options of an `…Items()` walker. */
export interface ItemsOptions extends RequestOptions {
  /** Stop after this many rows. */
  maxItems?: number
}

export interface HttpConfig {
  apiKey: string
  baseUrl: string
  chainId: number
  timeout: number
  maxRetries: number
  maxRetryWait: number
  userAgent?: string
  fetch?: typeof globalThis.fetch
}

type Query = object | undefined

const MAX_BACKOFF_MS = 8_000

/** A path value, percent-encoded. Empty, `.` and `..` would send the call elsewhere and are refused. */
export function seg(value: string | number): string {
  const s = String(value)
  if (s === '' || s === '.' || s === '..') throw new TypeError(`TraceFast: ${JSON.stringify(s)} is not a valid path value`)
  return encodeURIComponent(s)
}

/** The transport every method goes through. Not part of the public API. */
export class Http {
  readonly chainId: number
  readonly config: Readonly<HttpConfig>
  #headers: Record<string, string>

  constructor(config: HttpConfig) {
    this.config = config
    this.chainId = config.chainId
    this.#headers = {
      accept: 'application/json',
      'x-api-key': config.apiKey,
      'user-agent': `tracefast-typescript/${VERSION}${config.userAgent ? ` ${config.userAgent}` : ''}`,
    }
  }

  async get<T>(path: string, query: Query, options?: RequestOptions): Promise<T> {
    const url = this.#url(path, query)
    const maxRetries = options?.maxRetries ?? this.config.maxRetries
    for (let attempt = 0; ; attempt++) {
      let res: Response
      let text: string
      try {
        ;({ res, text } = await this.#attempt(url, options))
      } catch (e) {
        if (e instanceof Error && 'code' in e && (e as TraceFastError).code === 'connection_error' && attempt < maxRetries) {
          await sleep(backoff(attempt), options?.signal)
          continue
        }
        throw e
      }
      const requestId = res.headers.get('x-request-id') ?? undefined
      if (res.status >= 200 && res.status < 300) {
        try {
          return JSON.parse(text) as T
        } catch (cause) {
          throw makeError({ status: res.status, code: 'unexpected_response', message: 'the body is not JSON', requestId, cause })
        }
      }
      const error = toError(res, text, requestId)
      const wait = retryDelay(error, attempt, this.config.maxRetryWait)
      if (wait !== undefined && attempt < maxRetries) {
        await sleep(wait, options?.signal)
        continue
      }
      throw error
    }
  }

  async *pages<T>(path: string, query: Query, options?: PagesOptions): AsyncGenerator<T, void, undefined> {
    let params: Record<string, unknown> = { ...(query ?? {}) }
    const seen = new Set<string>()
    for (let n = 0; options?.maxPages === undefined || n < options.maxPages; n++) {
      const page = await this.get<T>(path, params, options)
      yield page
      const next = (page as { data?: { nextCursor?: unknown } }).data?.nextCursor
      if (next === undefined || next === null) return
      if (typeof next !== 'string' || next === '' || seen.has(next))
        throw makeError({ status: 200, code: 'unexpected_response', message: 'the answer repeated a cursor or gave an empty one' })
      seen.add(next)
      params = { ...params, cursor: next }
    }
  }

  async *items<R>(path: string, query: Query, field: string, options?: ItemsOptions): AsyncGenerator<R, void, undefined> {
    if (options?.maxItems !== undefined && options.maxItems <= 0) return
    let n = 0
    for await (const page of this.pages<{ data?: Record<string, unknown> }>(path, query, options)) {
      const rows = page.data?.[field]
      if (!Array.isArray(rows)) continue
      for (const row of rows as R[]) {
        yield row
        if (options?.maxItems !== undefined && ++n >= options.maxItems) return
      }
    }
  }

  #url(path: string, query: Query): string {
    const url = new URL(this.config.baseUrl.replace(/\/+$/, '') + path)
    for (const [k, v] of Object.entries(query ?? {})) {
      if (v === undefined || v === null) continue
      url.searchParams.set(k, String(v))
    }
    return url.toString()
  }

  /** One attempt: the request and its whole body, under one timer. */
  async #attempt(url: string, options?: RequestOptions): Promise<{ res: Response; text: string }> {
    const timeout = options?.timeout ?? this.config.timeout
    const timer = AbortSignal.timeout(timeout)
    const signal = options?.signal ? AbortSignal.any([options.signal, timer]) : timer
    const fetch = this.config.fetch ?? globalThis.fetch
    try {
      // A redirect is never followed: the key must not travel to another host (SPEC §1).
      const res = await fetch(url, { method: 'GET', headers: this.#headers, redirect: 'manual', signal })
      const text = await res.text()
      return { res, text }
    } catch (cause) {
      if (options?.signal?.aborted) throw makeError({ status: 0, code: 'aborted', message: 'the call was aborted', cause: options.signal.reason })
      if (timer.aborted) throw makeError({ status: 0, code: 'timeout', message: `no complete answer within ${timeout} ms`, cause })
      throw makeError({ status: 0, code: 'connection_error', message: `the connection to ${new URL(url).host} failed`, cause })
    }
  }
}

function toError(res: Response, text: string, headerRequestId: string | undefined): TraceFastError {
  const retryAfter = parseRetryAfter(res.headers.get('retry-after'))
  if (res.status >= 300 && res.status < 400)
    return makeError({ status: res.status, code: 'unexpected_response', message: 'a redirect, which is never followed', requestId: headerRequestId, retryAfter })
  let body: unknown
  try {
    body = JSON.parse(text)
  } catch {
    body = undefined
  }
  const e = (body as { error?: Record<string, unknown> } | undefined)?.error
  if (e && typeof e === 'object' && typeof e.code === 'string') {
    const details = e.details && typeof e.details === 'object' ? (e.details as Record<string, unknown>) : undefined
    const reason = typeof e.reason === 'string' ? e.reason : typeof details?.reason === 'string' ? details.reason : undefined
    return makeError({
      status: res.status,
      code: e.code,
      message: typeof e.message === 'string' ? e.message : e.code,
      reason,
      bucket: typeof e.bucket === 'string' ? e.bucket : undefined,
      details,
      requestId: typeof e.requestId === 'string' ? e.requestId : headerRequestId,
      retryAfter,
      body: e,
    })
  }
  return makeError({ status: res.status, code: 'unexpected_response', message: "an answer without the API's error body", requestId: headerRequestId, retryAfter })
}

/** How long to wait before retrying this error, or undefined when it is not retried (SPEC §5). */
function retryDelay(e: TraceFastError, attempt: number, maxRetryWaitS: number): number | undefined {
  if (e.status === 429) {
    if (e.code === 'quota_exceeded' || e.retryAfter === undefined || e.retryAfter > maxRetryWaitS) return undefined
    return e.retryAfter * 1000
  }
  if (e.status === 503) {
    if (e.retryAfter === undefined) return backoff(attempt)
    return e.retryAfter > maxRetryWaitS ? undefined : e.retryAfter * 1000
  }
  return undefined
}

function parseRetryAfter(h: string | null): number | undefined {
  if (!h) return undefined
  const s = Number(h)
  if (Number.isFinite(s) && s >= 0) return s
  const at = Date.parse(h)
  if (Number.isNaN(at)) return undefined
  return Math.max(0, Math.ceil((at - Date.now()) / 1000))
}

const backoff = (attempt: number) => Math.min(MAX_BACKOFF_MS, 500 * 2 ** attempt) * (0.5 + Math.random() / 2)

function sleep(ms: number, signal?: AbortSignal): Promise<void> {
  return new Promise((resolve, reject) => {
    const abort = () => reject(makeError({ status: 0, code: 'aborted', message: 'the call was aborted', cause: signal?.reason }))
    if (signal?.aborted) return abort()
    const t = setTimeout(resolve, ms)
    signal?.addEventListener('abort', () => (clearTimeout(t), abort()), { once: true })
  })
}
