# async but never awaits — should be a plain function.
async def fetch(url):
    data = build_request(url)
    return data
