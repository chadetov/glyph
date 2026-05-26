use reqwest::Client;

#[derive(Debug)]
pub struct Feed {
    pub user_id: String,
    pub posts: Vec<String>,
}

#[derive(Debug)]
pub enum FeedError {
    Network { status: u16 },
    Decode { reason: String },
}

pub async fn load_feed(client: &Client, user_id: &str) -> Result<Feed, FeedError> {
    let user_resp = client
        .get(format!("/api/users/{}", user_id))
        .send()
        .await
        .map_err(|e| FeedError::Decode { reason: e.to_string() })?;
    if !user_resp.status().is_success() {
        return Err(FeedError::Network { status: user_resp.status().as_u16() });
    }
    let posts_resp = client
        .get(format!("/api/users/{}/posts", user_id))
        .send()
        .await
        .map_err(|e| FeedError::Decode { reason: e.to_string() })?;
    if !posts_resp.status().is_success() {
        return Err(FeedError::Network { status: posts_resp.status().as_u16() });
    }
    #[derive(serde::Deserialize)]
    struct PostsData {
        titles: Vec<String>,
    }
    let posts_data: PostsData = posts_resp
        .json()
        .await
        .map_err(|e| FeedError::Decode { reason: e.to_string() })?;
    Ok(Feed { user_id: user_id.to_string(), posts: posts_data.titles })
}
