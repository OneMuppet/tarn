import asyncio

@decorator
def decorated():
    return 1

async def fetch(
    url,
    timeout=10,
):
    return url

def outer():
    def inner():
        return 2
    return inner()
