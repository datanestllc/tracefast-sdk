// Every example of the README, type-checked (`npm run typecheck`); not run.
import { TraceFast, TraceFastError } from '../src/index.ts'
import type { ProxyImplementationsResponse, AddressesTransactionsParams } from '../src/index.ts'

export async function readme(address: string, hash: string): Promise<void> {
  const tf = new TraceFast({ apiKey: process.env.TRACEFAST_API_KEY!, timeout: 30_000, maxRetries: 2, userAgent: 'my-app/1.4' })

  const { data: block } = await tf.blocks.latest()
  console.log(block.number, block.hash)

  const { data: caps } = await tf.contracts.capabilities('0xdac17f958d2ee523a2206206994597c13d831ec7')
  for (const c of caps.capabilities) {
    console.log(c.power, { inCode: !!c.inCode, availableNow: !!c.availableNow, usedBefore: !!c.usedBefore })
  }
  const { data: found } = await tf.resolve({ q: 'usdc' })
  void found

  const proxy: ProxyImplementationsResponse = await tf.contracts.proxy('0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48')
  for (const impl of proxy.data.implementations) {
    const state = impl.supersededBy === null ? 'current' : (impl.supersededBy ?? 'not stated')
    console.log(impl.implementation, impl.firstSeenBlock, state)
  }

  const { meta } = await tf.addresses.attributesAtBlock(address, { block: 20_000_000 })
  console.log(meta.asOfBlock, meta.docs)

  try {
    await tf.addresses.portfolio(address, { block: 30_000_000 })
  } catch (e) {
    if (e instanceof TraceFastError && e.code === 'block_not_available') console.log('covered up to', e.details?.maxAvailableBlock)
    else throw e
  }

  const controller = new AbortController()
  await tf.traces.get(hash, { includeCalldata: true }, { signal: controller.signal, timeout: 60_000 })

  const params: AddressesTransactionsParams = { limit: 100 }
  for await (const page of tf.addresses.transactionsPages(address, params)) {
    for (const tx of page.data.transactions) console.log(tx.hash)
  }
  await tf.blocks.atTime({ timestamp: 1767225600 })
  await tf.pools.forPair({ tokenA: '0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48', tokenB: '0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2' })
  await tf.tokens.price('0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48', { block: 20_000_000 })
  await tf.risk.blacklistStatus(address)
  await tf.consensus.validator(1)
}
