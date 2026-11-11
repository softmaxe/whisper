//! Bounded response bodies shared by network services.

#[derive(Debug, thiserror::Error)]
pub enum BodyError {
    #[error("{0}")]
    Network(#[from] reqwest::Error),
    #[error("Response exceeds {0} bytes")]
    TooLarge(usize),
}

pub async fn read_body(mut response: reqwest::Response, limit: usize) -> Result<String, BodyError> {
    if response
        .content_length()
        .is_some_and(|length| length > limit as u64)
    {
        return Err(BodyError::TooLarge(limit));
    }
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| BodyError::Network(error.without_url()))?
    {
        if chunk.len() > limit - body.len() {
            return Err(BodyError::TooLarge(limit));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(String::from_utf8_lossy(&body).into_owned())
}
