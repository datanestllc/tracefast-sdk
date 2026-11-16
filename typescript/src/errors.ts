/**
 * The stable words a TraceFast error can carry in `code`: the API's registry,
 * the gateway's refusals, and the SDK's own. Other values may appear.
 */
export type ErrorCode =
  | 'bad_request'
  | 'invalid_cursor'
  | 'not_found'
  | 'chain_not_supported'
  | 'method_not_allowed'
  | 'uri_too_long'
  | 'block_not_available'
  | 'coverage_gap'
  | 'portfolio_too_large'
  | 'pool_not_covered'
  | 'token_not_covered'
  | 'balance_not_available'
  | 'storage_parameters_unavailable'
  | 'code_too_large'
  | 'quota_exceeded'
  | 'service_unavailable'
  | 'internal_error'
  | 'auth_failed'
  | 'forbidden'
  | 'rate_limited'
  | 'concurrency_limited'
  | 'unexpected_response'
  | 'connection_error'
  | 'timeout'
  | 'aborted'
  | (string & {})

const BRAND = Symbol.for('tracefast.error')

/**
 * Every failure of a TraceFast call: a refusal from the API, a response that is
 * not the API's, a network failure, a timeout or an abort. Branch on `code`,
 * `reason` and the subclass — never on `message`, which is for humans.
 */
export class TraceFastError extends Error {
  /** HTTP status of the answer; `0` when no answer arrived. */
  readonly status: number
  /** The stable word to branch on. */
  readonly code: ErrorCode
  /**
   * The stable sub-code: `error.reason` on the gateway's key, plan and limit
   * refusals (`api_key_required`, `plan_scope_missing`, …), otherwise `details.reason`.
   */
  readonly reason?: string
  /** On a `429`: which limit refused (`base`, `class:…`, `concurrency`, `quota_daily`, `quota_method`, …). */
  readonly bucket?: string
  /** What to act on, when the API states it (`maxAvailableBlock`, `maxLimit`, `minimumPlan`, …). */
  readonly details?: Record<string, unknown>
  /** The request's id. Quote it when asking for support. */
  readonly requestId?: string
  /** Seconds the API asked to wait, from `Retry-After`, exactly as sent. */
  readonly retryAfter?: number
  /** The raw `error` object of the answer, so a field this SDK does not know survives. */
  readonly body?: Record<string, unknown>
  readonly [BRAND] = true

  constructor(init: {
    status: number
    code: string
    message: string
    reason?: string
    bucket?: string
    details?: Record<string, unknown>
    requestId?: string
    retryAfter?: number
    body?: Record<string, unknown>
    cause?: unknown
  }) {
    super(`${init.status ? `${init.status} ` : ''}${init.code}: ${init.message}`, init.cause === undefined ? undefined : { cause: init.cause })
    this.name = new.target.name
    this.status = init.status
    this.code = init.code
    this.reason = init.reason
    this.bucket = init.bucket
    this.details = init.details
    this.requestId = init.requestId
    this.retryAfter = init.retryAfter
    this.body = init.body
  }

  /** `true` for any TraceFast error — also one thrown by another copy of this package. */
  static is(e: unknown): e is TraceFastError {
    return typeof e === 'object' && e !== null && (e as Record<symbol, unknown>)[BRAND] === true
  }
}

/** `400` — the request is outside the contract; `reason` says which rule. */
export class BadRequestError extends TraceFastError {}
/** `401` — no key, or a key that is not valid. */
export class AuthenticationError extends TraceFastError {}
/** `403` — the key's plan does not include the method (`reason: plan_scope_missing`), or the key may not be used. */
export class PermissionDeniedError extends TraceFastError {}
/** `404` — no object at the key the path names, or no such method. */
export class NotFoundError extends TraceFastError {}
/** `422` — well-formed, but the data cannot answer it (`block_not_available`, `coverage_gap`, …). */
export class UnprocessableEntityError extends TraceFastError {}
/** `429` — a rate limit, a concurrency cap or a quota; `bucket` says which. */
export class RateLimitError extends TraceFastError {}
/** `5xx` — on our side. */
export class InternalServerError extends TraceFastError {}
/** No answer: the connection failed or broke. */
export class APIConnectionError extends TraceFastError {}
/** No answer within the timeout. */
export class APITimeoutError extends TraceFastError {}
/** An answer that is not the API's: a redirect, a proxy page, a body that is not JSON. */
export class UnexpectedResponseError extends TraceFastError {}
/** The caller's abort signal fired. */
export class RequestAbortedError extends TraceFastError {}

/** The subclass for an answer's status or the SDK's own code. */
export function errorClassFor(status: number, code: string): typeof TraceFastError {
  if (code === 'unexpected_response') return UnexpectedResponseError
  if (code === 'connection_error') return APIConnectionError
  if (code === 'timeout') return APITimeoutError
  if (code === 'aborted') return RequestAbortedError
  if (status === 400) return BadRequestError
  if (status === 401) return AuthenticationError
  if (status === 403) return PermissionDeniedError
  if (status === 404) return NotFoundError
  if (status === 422) return UnprocessableEntityError
  if (status === 429) return RateLimitError
  if (status >= 500) return InternalServerError
  return TraceFastError
}

/** Build the right subclass. */
export function makeError(init: ConstructorParameters<typeof TraceFastError>[0]): TraceFastError {
  const Cls = errorClassFor(init.status, init.code)
  return new Cls(init)
}
