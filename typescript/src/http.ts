import { TraceFastError } from './errors.ts'
import { VERSION } from './version.ts'

/** Per-call options. */
export interface RequestOptions {
  /** Cancels the call. */
  signal?: AbortSignal
  /** Overrides the client's timeout for this call, in milliseconds. */
  timeout?: number
}

export interface HttpConfig {
  apiKey: string
  baseUrl: string
  chainId: number
  timeout: number
  maxRetries: number
  userAgent?: string
  fetch: typeof globalThis.fetch
}

type Query = object | undefined

const MAX_RETRY_AFTER_S = 60

/** The transport every method goes through. Not part of the public API. */
export class Http {
  readonly chainId: number
  #config: HttpConfig

  constructor(config: HttpConfig) {
    this.#config = config
    this.chainId = config.chainId
  }

  async get<T>(path: string, query: Query, options?: RequestOptions): Promise<T> {
    const url = this.#url(path, query)
    for (let attempt = 0; ; attempt++) {
      let res: Response
      try {
        res = await this.#fetch(url, options)
      } catch (e) {
        if (e instanceof TraceFastError && e.code === 'timeout') throw e
        if (options?.signal?.aborted) throw e
        if (attempt < this.#config.maxRetries) {
          await sleep(backoff(attempt), options?.signal)
          continue
        }
        throw new TraceFastError({ status: 0, code: 'connection_error', message: `could not reach ${this.#config.baseUrl}`, cause: e })
      }
      if (res.status >= 200 && res.status < 300) {
        const text = await res.text()
        try {
          return JSON.parse(text) as T
        } catch (e) {
          throw new TraceFastError({ status: res.status, code: 'unexpected_response', message: `HTTP ${res.status} with a body that is not JSON`, requestId: res.headers.get('x-request-id') ?? undefined, cause: e })
        }
      }
      const error = await toError(res)
      const wait = retryDelay(res.status, error.retryAfter, attempt)
      if (wait !== undefined && attempt < this.#config.maxRetries) {
        await sleep(wait, options?.signal)
        continue
      }
      throw error
    }
  }

  async *pages<T>(path: string, query: Query, options?: RequestOptions): AsyncGenerator<T, void, undefined> {
    let params: Record<string, unknown> = { ...(query ?? {}) }
    for (;;) {
      const page = await this.get<T>(path, params, options)
      yield page
      const next = (page as { data?: { nextCursor?: unknown } }).data?.nextCursor
      if (typeof next !== 'string' || next === '') return
      params = { ...params, cursor: next }
    }
  }

  #url(path: string, query: Query): string {
    const url = new URL(this.#config.baseUrl.replace(/\/+$/, '') + path)
    for (const [k, v] of Object.entries(query ?? {})) {
      if (v === undefined || v === null) continue
      url.searchParams.set(k, String(v))
    }
    return url.toString()
  }

  async #fetch(url: string, options?: RequestOptions): Promise<Response> {
    const timeout = options?.timeout ?? this.#config.timeout
    const timer = AbortSignal.timeout(timeout)
    const signal = options?.signal ? AbortSignal.any([options.signal, timer]) : timer
    try {
      return await this.#config.fetch(url, {
        method: 'GET',
        // A redirect is never followed: the key must not travel to another host (SPEC §1).
        redirect: 'manual',
        headers: {
          accept: 'application/json',
          'x-api-key': this.#config.apiKey,
          'user-agent': `tracefast-typescript/${VERSION}${this.#config.userAgent ? ` ${this.#config.userAgent}` : ''}`,
        },
        signal,
      })
    } catch (e) {
      if (timer.aborted && !options?.signal?.aborted)
        throw new TraceFastError({ status: 0, code: 'timeout', message: `no answer within ${timeout} ms`, cause: e })
      throw e
    }
  }
}

async function toError(res: Response): Promise<TraceFastError> {
  const retryAfter = parseRetryAfter(res.headers.get('retry-after'))
  const text = await res.text().catch(() => '')
  let body: unknown
  try {
    body = JSON.parse(text)
  } catch {
    body = undefined
  }
  const e = (body as { error?: Record<string, unknown> } | undefined)?.error
  if (e && typeof e.code === 'string') {
    return new TraceFastError({
      status: res.status,
      code: e.code,
      message: typeof e.message === 'string' ? e.message : e.code,
      details: e.details && typeof e.details === 'object' ? (e.details as Record<string, unknown>) : undefined,
      requestId: typeof e.requestId === 'string' ? e.requestId : (res.headers.get('x-request-id') ?? undefined),
      retryAfter,
    })
  }
  return new TraceFastError({
    status: res.status,
    code: 'unexpected_response',
    message: `HTTP ${res.status} without the API's error body`,
    requestId: res.headers.get('x-request-id') ?? undefined,
    retryAfter,
  })
}

/** How long to wait before retrying this answer, or undefined if it is not retried (SPEC §5). */
function retryDelay(status: number, retryAfter: number | undefined, attempt: number): number | undefined {
  if (status === 429) return retryAfter === undefined ? undefined : retryAfter * 1000
  if (status === 503) return retryAfter === undefined ? backoff(attempt) : retryAfter * 1000
  return undefined
}

function parseRetryAfter(h: string | null): number | undefined {
  if (!h) return undefined
  const s = Number(h)
  if (Number.isFinite(s) && s >= 0) return Math.min(s, MAX_RETRY_AFTER_S)
  const at = Date.parse(h)
  if (Number.isNaN(at)) return undefined
  return Math.min(Math.max(0, Math.ceil((at - Date.now()) / 1000)), MAX_RETRY_AFTER_S)
}

const backoff = (attempt: number) => 500 * 2 ** attempt * (0.5 + Math.random() / 2)

function sleep(ms: number, signal?: AbortSignal): Promise<void> {
  return new Promise((resolve, reject) => {
    if (signal?.aborted) return reject(signal.reason)
    const t = setTimeout(resolve, ms)
    signal?.addEventListener('abort', () => (clearTimeout(t), reject(signal.reason)), { once: true })
  })
}
