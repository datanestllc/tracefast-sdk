import { Http } from './http.ts'
import {
  AddressesApi,
  BlocksApi,
  ConsensusApi,
  ContractsApi,
  PoolsApi,
  RiskApi,
  RootApi,
  TokensApi,
  TracesApi,
  TransactionsApi,
} from './generated/api.ts'

export interface TraceFastOptions {
  /** Your TraceFast API key (`tf_live_…`). Server-side only — never ship it to a browser. */
  apiKey: string
  /** Default `https://tracefast.xyz/api/v1`. */
  baseUrl?: string
  /** Default `1`, Ethereum mainnet — the only chain served today. */
  chainId?: number
  /** Per-call timeout in milliseconds. Default 30 000. */
  timeout?: number
  /** Retries of answers that say "try again" (`429` with `Retry-After`, `503`, a connection failure). Default 2. */
  maxRetries?: number
  /** Appended to the `User-Agent` header, e.g. `my-app/1.2`. */
  userAgent?: string
  /** A `fetch` to use instead of the global one. */
  fetch?: typeof globalThis.fetch
}

/**
 * The TraceFast API client.
 *
 * ```ts
 * const tf = new TraceFast({ apiKey: process.env.TRACEFAST_API_KEY! })
 * const { data, meta } = await tf.contracts.proxy('0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48')
 * ```
 */
export class TraceFast {
  readonly addresses: AddressesApi
  readonly blocks: BlocksApi
  readonly consensus: ConsensusApi
  readonly contracts: ContractsApi
  readonly pools: PoolsApi
  readonly risk: RiskApi
  readonly tokens: TokensApi
  readonly traces: TracesApi
  readonly transactions: TransactionsApi
  /** Resolve a name, ENS name, token symbol or address to the addresses it can mean. */
  readonly resolve: RootApi['resolve']

  constructor(options: TraceFastOptions) {
    if (!options?.apiKey) throw new TypeError('TraceFast: apiKey is required')
    const http = new Http({
      apiKey: options.apiKey,
      baseUrl: options.baseUrl ?? 'https://tracefast.xyz/api/v1',
      chainId: options.chainId ?? 1,
      timeout: options.timeout ?? 30_000,
      maxRetries: options.maxRetries ?? 2,
      userAgent: options.userAgent,
      fetch: options.fetch ?? globalThis.fetch.bind(globalThis),
    })
    this.addresses = new AddressesApi(http)
    this.blocks = new BlocksApi(http)
    this.consensus = new ConsensusApi(http)
    this.contracts = new ContractsApi(http)
    this.pools = new PoolsApi(http)
    this.risk = new RiskApi(http)
    this.tokens = new TokensApi(http)
    this.traces = new TracesApi(http)
    this.transactions = new TransactionsApi(http)
    const root = new RootApi(http)
    this.resolve = root.resolve.bind(root)
  }
}
