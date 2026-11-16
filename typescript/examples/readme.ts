// Every example of the README, type-checked by `npm run typecheck`; not run.
import { TraceFast, PermissionDeniedError, UnprocessableEntityError } from '../src/index.ts'
import type { ContractsProxyResponse, AddressesTransactionsParams } from '../src/index.ts'

export async function readme(hash: string, tokenA: string, tokenB: string, pool: string, address: string): Promise<void> {
  const tf = new TraceFast({ apiKey: process.env.TRACEFAST_API_KEY! })

  const { data: block } = await tf.blocks.latest()
  console.log(block.number, block.hash)

  const { data: proxy } = await tf.contracts.proxy('0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48')
  for (const impl of proxy.implementations) {
    const state = impl.supersededBy === null ? 'current' : (impl.supersededBy ?? 'not stated')
    console.log(impl.implementation, 'since block', impl.firstSeenBlock, state)
  }

  const { data: tx } = await tf.transactions.get('0x6bb3103eec4ca8f4a324bbab71dc84f533af7e6be11ab8b13d0d10333bb9abc5')
  console.log(tx.from, '→', tx.to, tx.valueWei, 'wei')

  try {
    await tf.contracts.capabilities('0xdac17f958d2ee523a2206206994597c13d831ec7')
  } catch (e) {
    if (e instanceof PermissionDeniedError && e.reason === 'plan_scope_missing') {
      console.log('needs', e.details?.minimumPlan, '— this key has', e.details?.currentPlan)
    } else throw e
  }

  const configured = new TraceFast({ apiKey: process.env.TRACEFAST_API_KEY!, timeout: 30_000, maxRetries: 2, userAgent: 'my-app/1.4' })
  const patient = configured.withOptions({ timeout: 120_000, maxRetries: 4 })
  void patient

  await tf.blocks.get(25_920_000)
  await tf.blocks.atTime({ timestamp: 1767225600 })
  await tf.blocks.transactions(25_920_000, { limit: 100 })
  await tf.transactions.events(hash)
  await tf.transactions.failures(hash)
  await tf.transactions.storageWrites(hash)
  await tf.traces.get(hash, { includeCalldata: true })
  await tf.traces.frames(hash, { limit: 100 })
  await tf.contracts.get(address)
  await tf.contracts.bytecode(address)
  await tf.tokens.get(address)
  await tf.tokens.holders(address)
  await tf.tokens.price(address, { block: 20_000_000 })
  await tf.tokens.transfers(address)
  await tf.addresses.get(address)
  await tf.addresses.portfolio(address)
  await tf.addresses.attributes(address)
  await tf.addresses.transactions(address, { direction: 'sent' })
  await tf.pools.get(pool)
  await tf.pools.forPair({ tokenA, tokenB })
  await tf.pools.swaps(pool)
  await tf.pools.reserves(pool)
  await tf.consensus.current()
  await tf.consensus.queues()
  await tf.consensus.validator(1)
  await tf.consensus.missedSlots()
  await tf.risk.blacklistStatus(address)
  await tf.risk.blacklistEvents(address)
  await tf.resolve({ q: 'usdc' })

  const { meta } = await tf.blocks.latest()
  console.log(meta.asOfBlock, meta.docs)

  try {
    await tf.tokens.price('0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48', { block: 99_000_000 })
  } catch (e) {
    if (e instanceof UnprocessableEntityError && e.code === 'block_not_available') console.log('covered up to', e.details?.maxAvailableBlock)
    else throw e
  }

  const controller = new AbortController()
  const trace = tf.traces.get(hash, {}, { signal: controller.signal, timeout: 60_000 })
  controller.abort()
  void trace

  const vitalik = '0xd8da6bf26964af9d7eed9e03e53415d37aa96045'
  for await (const t of tf.addresses.transactionsItems(vitalik, { limit: 100 }, { maxItems: 500 })) console.log(t.hash)
  for await (const page of tf.blocks.transactionsPages(25_920_000, { limit: 100 }, { maxPages: 3 })) console.log(page.data.transactions.length)

  const typed: ContractsProxyResponse = await tf.contracts.proxy(address)
  const params: AddressesTransactionsParams = { limit: 100 }
  void typed
  void params
}
