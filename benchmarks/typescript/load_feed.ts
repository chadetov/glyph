type Feed = {
  user_id: string;
  posts: string[];
};

type FeedError =
  | { kind: "network"; status: number }
  | { kind: "decode"; reason: string };

type FeedResult =
  | { ok: true; value: Feed }
  | { ok: false; error: FeedError };

export async function loadFeed(userId: string): Promise<FeedResult> {
  try {
    const userResp = await fetch(`/api/users/${userId}`);
    if (!userResp.ok) {
      return { ok: false, error: { kind: "network", status: userResp.status } };
    }
    const postsResp = await fetch(`/api/users/${userId}/posts`);
    if (!postsResp.ok) {
      return { ok: false, error: { kind: "network", status: postsResp.status } };
    }
    const postsData = (await postsResp.json()) as { titles: string[] };
    return {
      ok: true,
      value: { user_id: userId, posts: postsData.titles },
    };
  } catch (e) {
    return { ok: false, error: { kind: "decode", reason: String(e) } };
  }
}
