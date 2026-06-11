# Recall guard for the masking change: a REAL blocking call in code (not in a
# string or comment) must still be flagged by the sync/async-conflict check.
import asyncio


async def fetch_everything(urls):
    results = []
    for u in urls:
        data = requests.get(u)
        results.append(data)
    return results
