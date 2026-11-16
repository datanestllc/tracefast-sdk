/**
 * Every failure of a TraceFast call: a refusal from the API, a response that is
 * not the API's, a network failure or a timeout. Branch on `code` and
 * `details.reason` — never on `message`, which is for humans and may be reworded.
 */
export class TraceFastError extends Error {
  /** HTTP status of the answer; `0` when no answer arrived. */
  readonly status: number
  /**
   * The stable word from the API's error registry (`bad_request`, `not_found`,
   * `block_not_available`, `rate_limited`, …), or one of the SDK's own:
   * `unexpected_response`, `connection_error`, `timeout`.
   */
  readonly code: string
  /** What to act on, when the API states it — `details.reason` is the stable sub-code. */
  readonly details?: Record<string, unknown>
  /** The request's id. Quote it when asking for support. */
  readonly requestId?: string
  /** Seconds the API asked to wait, from `Retry-After`. */
  readonly retryAfter?: number

  constructor(init: {
    status: number
    code: string
    message: string
    details?: Record<string, unknown>
    requestId?: string
    retryAfter?: number
    cause?: unknown
  }) {
    super(init.message, init.cause === undefined ? undefined : { cause: init.cause })
    this.name = 'TraceFastError'
    this.status = init.status
    this.code = init.code
    this.details = init.details
    this.requestId = init.requestId
    this.retryAfter = init.retryAfter
  }

  /** `details.reason`, when the API gave one. */
  get reason(): string | undefined {
    const r = this.details?.reason
    return typeof r === 'string' ? r : undefined
  }
}
