# Regression: a blocking-call keyword that appears only inside a comment or a
# string literal must NOT trigger the sync/async-conflict check. The async fn
# below is clean; `requests.get(` and `time.sleep(` occur only as text.
import asyncio


async def fetch_everything(urls):
    # NOTE: never call requests.get( inside an async function — it blocks.
    hint = "the sync version used time.sleep( here"
    results = []
    for u in urls:
        results.append(await fetch_one(u))
    return results


async def fetch_one(u):
    await asyncio.sleep(0)
    return u
