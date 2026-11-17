import { Http, type HttpConfig } from './http.ts'
import { ClientBase } from './generated/api.ts'

export interface TraceFastOptions {
  /** Your TraceFast API key (`tf_live_…`). Server-side only — never ship it to a browser. */
  apiKey: string
  /** Default `https://tracefast.xyz/api/v1`. Must be `https` (a loopback host may use `http`). */
  baseUrl?: string
  /** Default `1`, Ethereum mainnet — the only chain served today. */
  chainId?: number
  /** Milliseconds per attempt, the body included. Default 30 000. */
  timeout?: number
  /** Retries of answers that say "try again" (`429` with a short `Retry-After`, `503`, a connection failure). Default 2. */
  maxRetries?: number
  /** The longest `Retry-After`, in seconds, the client waits out before retrying. Default 60. */
  maxRetryWait?: number
  /** Appended to the `User-Agent` header, e.g. `my-app/1.2`. */
  userAgent?: string
  /** A `fetch` to use instead of the global one (read at each call). */
  fetch?: typeof globalThis.fetch
}

const LOOPBACK = new Set(['localhost', '127.0.0.1', '[::1]'])

function validate(o: TraceFastOptions): HttpConfig {
  if (typeof o?.apiKey !== 'string' || !/^[\x21-\x7E]+$/.test(o.apiKey))
    throw new TypeError('TraceFast: apiKey is required and may hold only visible ASCII characters — check for a space, a line break or a typographic character')
  const baseUrl = o.baseUrl ?? 'https://tracefast.xyz/api/v1'
  let url: URL
  try {
    url = new URL(baseUrl)
  } catch {
    throw new TypeError('TraceFast: baseUrl is not a URL')
  }
  if (url.hash || url.search) throw new TypeError('TraceFast: baseUrl must not carry a query or a fragment')
  if (url.protocol !== 'https:' && !(url.protocol === 'http:' && LOOPBACK.has(url.hostname)))
    throw new TypeError('TraceFast: baseUrl must use https')
  const timeout = o.timeout ?? 30_000
  if (!Number.isFinite(timeout) || timeout <= 0 || timeout > 2_147_483_647) throw new RangeError('TraceFast: timeout must be a positive number of milliseconds')
  const maxRetries = o.maxRetries ?? 2
  if (!Number.isInteger(maxRetries) || maxRetries < 0) throw new RangeError('TraceFast: maxRetries must be a whole number, 0 or more')
  const maxRetryWait = o.maxRetryWait ?? 60
  if (!Number.isFinite(maxRetryWait) || maxRetryWait < 0) throw new RangeError('TraceFast: maxRetryWait must be a number of seconds, 0 or more')
  if (!o.fetch && typeof globalThis.fetch !== 'function') throw new TypeError('TraceFast: no global fetch — pass one in options.fetch')
  return {
    apiKey: o.apiKey,
    baseUrl,
    chainId: o.chainId ?? 1,
    timeout,
    maxRetries,
    maxRetryWait,
    userAgent: o.userAgent,
    fetch: o.fetch,
  }
}

/**
 * The TraceFast API client.
 *
 * ```ts
 * const tf = new TraceFast({ apiKey: process.env.TRACEFAST_API_KEY! })
 * const { data, meta } = await tf.contracts.proxy('0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48')
 * ```
 */
export class TraceFast extends ClientBase {
  #options: TraceFastOptions

  constructor(options: TraceFastOptions) {
    super(new Http(validate(options)))
    this.#options = { ...options }
  }

  /**
   * A copy of this client with some options changed — for one part of a program.
   *
   * ```ts
   * const patient = tf.withOptions({ timeout: 120_000, maxRetries: 4 })
   * ```
   */
  withOptions(changes: Partial<Omit<TraceFastOptions, 'apiKey'>>): TraceFast {
    return new TraceFast({ ...this.#options, ...changes })
  }

  /** Keeps the key out of logs. */
  toJSON(): Record<string, unknown> {
    return { baseUrl: this.#options.baseUrl ?? 'https://tracefast.xyz/api/v1', chainId: this.#options.chainId ?? 1 }
  }
}
