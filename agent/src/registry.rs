use std::{
    collections::HashMap,
    sync::Arc,
};

use anyhow::Result;
use chrono::{DateTime, Utc};
use reqwest::header;
use tokio::sync::Mutex;
use tracing::{debug, info};

pub struct VersionCheckResult {
    pub latest_digest: String,
    pub update_available: bool,
}

struct CachedDigest {
    digest: String,
    fetched_at: DateTime<Utc>,
}

pub struct RegistryChecker {
    cache: Arc<Mutex<HashMap<String, CachedDigest>>>,
    client: reqwest::Client,
}

impl RegistryChecker {
    pub fn new() -> Self {
        let mut headers = header::HeaderMap::new();
        headers.insert(
            header::USER_AGENT,
            header::HeaderValue::from_static("dockup-agent/0.1.0"),
        );
        let client = reqwest::Client::builder()
            .default_headers(headers)
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("Failed to build HTTP client");

        Self {
            cache: Arc::new(Mutex::new(HashMap::new())),
            client,
        }
    }

    pub async fn check_image(
        &self,
        current_image: &str,
        current_digest: Option<&str>,
    ) -> Result<VersionCheckResult> {
        let (registry, repo, tag) = parse_image(current_image);

        // Check cache (1-hour TTL)
        {
            let cache = self.cache.lock().await;
            if let Some(cached) = cache.get(current_image) {
                let age = Utc::now()
                    .signed_duration_since(cached.fetched_at)
                    .num_minutes();
                if age < 60 {
                    debug!("Cache hit for {} (age {}min)", current_image, age);
                    let latest = cached.digest.clone();
                    let update_available = current_digest
                        .map(|d| d != latest)
                        .unwrap_or(false);
                    return Ok(VersionCheckResult {
                        latest_digest: latest,
                        update_available,
                    });
                }
            }
        }

        info!("Checking registry for image: {}", current_image);

        let latest_digest = match registry.as_str() {
            "docker.io" | "registry-1.docker.io" => {
                self.check_dockerhub(&repo, &tag).await?
            }
            r => {
                self.check_oci_registry(r, &repo, &tag).await?
            }
        };

        // Update cache
        {
            let mut cache = self.cache.lock().await;
            cache.insert(
                current_image.to_string(),
                CachedDigest {
                    digest: latest_digest.clone(),
                    fetched_at: Utc::now(),
                },
            );
        }

        let update_available = current_digest
            .map(|d| d != latest_digest)
            .unwrap_or(false);

        Ok(VersionCheckResult {
            latest_digest,
            update_available,
        })
    }

    async fn check_dockerhub(&self, repo: &str, tag: &str) -> Result<String> {
        // Docker Hub: use the registry API with token auth
        let token_url = format!(
            "https://auth.docker.io/token?service=registry.docker.io&scope=repository:{}:pull",
            repo
        );

        let token_resp: serde_json::Value = self
            .client
            .get(&token_url)
            .send()
            .await?
            .json()
            .await?;

        let token = token_resp
            .get("token")
            .and_then(|t| t.as_str())
            .ok_or_else(|| anyhow::anyhow!("No token in Docker Hub response"))?;

        let manifest_url = format!(
            "https://registry-1.docker.io/v2/{}/manifests/{}",
            repo, tag
        );

        let resp = self
            .client
            .get(&manifest_url)
            .bearer_auth(token)
            .header(
                "Accept",
                "application/vnd.oci.image.index.v1+json,application/vnd.docker.distribution.manifest.v2+json,application/vnd.docker.distribution.manifest.list.v2+json",
            )
            .send()
            .await?;

        if let Some(digest) = resp.headers().get("Docker-Content-Digest") {
            Ok(digest.to_str()?.to_string())
        } else {
            // Fallback: use Hub API to get digest
            debug!("No Docker-Content-Digest header for {}", repo);
            let hub_url = format!(
                "https://hub.docker.com/v2/repositories/{}/tags/{}",
                repo, tag
            );
            let hub_resp: serde_json::Value = self
                .client
                .get(&hub_url)
                .send()
                .await?
                .json()
                .await?;

            let digest = hub_resp
                .pointer("/images/0/digest")
                .or_else(|| hub_resp.get("digest"))
                .and_then(|d| d.as_str())
                .ok_or_else(|| anyhow::anyhow!("Could not determine digest from Docker Hub"))?;

            Ok(digest.to_string())
        }
    }

    async fn check_oci_registry(
        &self,
        registry: &str,
        repo: &str,
        tag: &str,
    ) -> Result<String> {
        let manifest_url = format!(
            "https://{}/v2/{}/manifests/{}",
            registry, repo, tag
        );

        let resp = self
            .client
            .head(&manifest_url)
            .header(
                "Accept",
                "application/vnd.oci.image.index.v1+json,application/vnd.docker.distribution.manifest.v2+json,application/vnd.docker.distribution.manifest.list.v2+json",
            )
            .send()
            .await;

        match resp {
            Ok(r) if r.status().is_success() => {
                if let Some(digest) = r.headers().get("Docker-Content-Digest") {
                    Ok(digest.to_str()?.to_string())
                } else {
                    // Try GET if HEAD didn't return digest
                    let get_resp = self
                        .client
                        .get(&manifest_url)
                        .header(
                            "Accept",
                            "application/vnd.oci.image.index.v1+json,application/vnd.docker.distribution.manifest.v2+json,application/vnd.docker.distribution.manifest.list.v2+json",
                        )
                        .send()
                        .await?;

                    if let Some(digest) = get_resp.headers().get("Docker-Content-Digest") {
                        Ok(digest.to_str()?.to_string())
                    } else {
                        anyhow::bail!("No Docker-Content-Digest header from {}", registry)
                    }
                }
            }
            Ok(r) if r.status() == 401 => {
                // Try with www-authenticate challenge parsing
                let www_auth = r
                    .headers()
                    .get("www-authenticate")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("")
                    .to_string();

                let token = obtain_registry_token(&self.client, &www_auth).await?;

                let auth_resp = self
                    .client
                    .head(&manifest_url)
                    .bearer_auth(&token)
                    .header(
                        "Accept",
                        "application/vnd.oci.image.index.v1+json,application/vnd.docker.distribution.manifest.v2+json,application/vnd.docker.distribution.manifest.list.v2+json",
                    )
                    .send()
                    .await?;

                if let Some(digest) = auth_resp.headers().get("Docker-Content-Digest") {
                    Ok(digest.to_str()?.to_string())
                } else {
                    anyhow::bail!("No Docker-Content-Digest header after auth from {}", registry)
                }
            }
            Ok(r) => {
                anyhow::bail!(
                    "Registry {} returned status {} for {}",
                    registry,
                    r.status(),
                    manifest_url
                )
            }
            Err(e) => Err(e.into()),
        }
    }
}

async fn obtain_registry_token(client: &reqwest::Client, www_auth: &str) -> Result<String> {
    // Parse Bearer realm="...",service="...",scope="..."
    let realm = extract_param(www_auth, "realm").unwrap_or_default();
    let service = extract_param(www_auth, "service").unwrap_or_default();
    let scope = extract_param(www_auth, "scope").unwrap_or_default();

    let token_url = format!(
        "{}?service={}&scope={}",
        realm, service, scope
    );

    let resp: serde_json::Value = client.get(&token_url).send().await?.json().await?;

    let token = resp
        .get("token")
        .or_else(|| resp.get("access_token"))
        .and_then(|t| t.as_str())
        .ok_or_else(|| anyhow::anyhow!("No token in registry auth response"))?;

    Ok(token.to_string())
}

fn extract_param(header: &str, key: &str) -> Option<String> {
    let prefix = format!("{}=\"", key);
    let start = header.find(&prefix)?;
    let rest = &header[start + prefix.len()..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

pub fn parse_image(image: &str) -> (String, String, String) {
    // Remove digest if present
    let image = if let Some(idx) = image.find('@') {
        &image[..idx]
    } else {
        image
    };

    // Extract tag
    let (image_no_tag, tag) = if let Some(idx) = image.rfind(':') {
        // Make sure this colon is not in a registry host (e.g. localhost:5000/img)
        let before = &image[..idx];
        if before.contains('/') || !before.contains(':') {
            (&image[..idx], image[idx + 1..].to_string())
        } else {
            (image, "latest".to_string())
        }
    } else {
        (image, "latest".to_string())
    };

    // Check if image has a registry host (contains a dot or colon or is 'localhost')
    let parts: Vec<&str> = image_no_tag.splitn(2, '/').collect();
    let (registry, repo) = if parts.len() == 2
        && (parts[0].contains('.') || parts[0].contains(':') || parts[0] == "localhost")
    {
        (parts[0].to_string(), parts[1].to_string())
    } else if parts.len() == 1 {
        // Official Docker Hub image like "nginx"
        (
            "docker.io".to_string(),
            format!("library/{}", parts[0]),
        )
    } else {
        // Docker Hub user/repo
        ("docker.io".to_string(), image_no_tag.to_string())
    };

    (registry, repo, tag)
}
