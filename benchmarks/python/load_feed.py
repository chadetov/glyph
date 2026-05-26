from dataclasses import dataclass

import httpx


@dataclass
class Feed:
    user_id: str
    posts: list[str]


@dataclass
class NetworkError:
    status: int


@dataclass
class DecodeError:
    reason: str


FeedError = NetworkError | DecodeError


@dataclass
class FeedOk:
    value: Feed


@dataclass
class FeedErr:
    error: FeedError


async def load_feed(user_id: str) -> FeedOk | FeedErr:
    async with httpx.AsyncClient() as client:
        try:
            user_resp = await client.get(f"/api/users/{user_id}")
            if user_resp.status_code >= 400:
                return FeedErr(NetworkError(status=user_resp.status_code))
            posts_resp = await client.get(f"/api/users/{user_id}/posts")
            if posts_resp.status_code >= 400:
                return FeedErr(NetworkError(status=posts_resp.status_code))
            posts_data = posts_resp.json()
            return FeedOk(Feed(user_id=user_id, posts=posts_data["titles"]))
        except Exception as e:
            return FeedErr(DecodeError(reason=str(e)))
