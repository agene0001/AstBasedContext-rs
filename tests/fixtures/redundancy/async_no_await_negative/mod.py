# Properly awaits — must not be flagged.
async def fetch(url):
    data = await get(url)
    return data
