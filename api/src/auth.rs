use anyhow::Result;
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,
    pub iat: usize,
}

pub fn generate_agent_jwt(client_id: &str, secret: &str) -> Result<String> {
    let claims = Claims {
        sub: client_id.to_string(),
        iat: chrono::Utc::now().timestamp() as usize,
    };
    let token = encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )?;
    Ok(token)
}

pub fn validate_agent_jwt(token: &str, secret: &str) -> Result<String> {
    let mut validation = Validation::new(Algorithm::HS256);
    validation.validate_exp = false;
    validation.required_spec_claims.clear();
    let data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &validation,
    )?;
    Ok(data.claims.sub)
}
