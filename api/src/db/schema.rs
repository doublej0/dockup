use anyhow::Result;
use sqlx::SqlitePool;

use crate::models::{Client, Container, UpdateJob};

pub async fn get_all_clients(pool: &SqlitePool) -> Result<Vec<Client>> {
    let rows = sqlx::query!(
        r#"SELECT id, name, host, color, compose_file_path, agent_version, agent_update_mode,
                  last_seen, connected as "connected: i64", created_at FROM clients ORDER BY created_at DESC"#
    )
    .fetch_all(pool)
    .await?;

    let clients = rows
        .into_iter()
        .map(|r| Client {
            id: r.id,
            name: r.name,
            host: r.host,
            color: r.color,
            compose_file_path: r.compose_file_path,
            agent_version: r.agent_version,
            agent_update_mode: r.agent_update_mode,
            last_seen: r.last_seen,
            connected: r.connected != 0,
            created_at: r.created_at,
        })
        .collect();

    Ok(clients)
}

pub async fn get_client(pool: &SqlitePool, id: &str) -> Result<Option<Client>> {
    let row = sqlx::query!(
        r#"SELECT id, name, host, color, compose_file_path, agent_version, agent_update_mode,
                  last_seen, connected as "connected: i64", created_at FROM clients WHERE id = ?"#,
        id
    )
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| Client {
        id: r.id,
        name: r.name,
        host: r.host,
        color: r.color,
        compose_file_path: r.compose_file_path,
        agent_version: r.agent_version,
        agent_update_mode: r.agent_update_mode,
        last_seen: r.last_seen,
        connected: r.connected != 0,
        created_at: r.created_at,
    }))
}

pub async fn insert_client(pool: &SqlitePool, client: &Client) -> Result<()> {
    let connected: i64 = if client.connected { 1 } else { 0 };
    sqlx::query!(
        r#"INSERT INTO clients (id, name, host, color, compose_file_path, agent_version, agent_update_mode, last_seen, connected, created_at)
           VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
        client.id,
        client.name,
        client.host,
        client.color,
        client.compose_file_path,
        client.agent_version,
        client.agent_update_mode,
        client.last_seen,
        connected,
        client.created_at,
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn update_client(
    pool: &SqlitePool,
    id: &str,
    name: &str,
    color: &str,
    compose_file_path: Option<&str>,
    agent_update_mode: &str,
) -> Result<()> {
    sqlx::query!(
        r#"UPDATE clients SET name = ?, color = ?, compose_file_path = ?, agent_update_mode = ? WHERE id = ?"#,
        name,
        color,
        compose_file_path,
        agent_update_mode,
        id,
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn delete_client(pool: &SqlitePool, id: &str) -> Result<()> {
    sqlx::query!("DELETE FROM clients WHERE id = ?", id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn set_client_connected(
    pool: &SqlitePool,
    id: &str,
    connected: bool,
    last_seen: Option<&str>,
) -> Result<()> {
    let connected_int: i64 = if connected { 1 } else { 0 };
    sqlx::query!(
        "UPDATE clients SET connected = ?, last_seen = ? WHERE id = ?",
        connected_int,
        last_seen,
        id,
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn set_client_agent_version(pool: &SqlitePool, id: &str, version: &str) -> Result<()> {
    sqlx::query!(
        "UPDATE clients SET agent_version = ? WHERE id = ?",
        version,
        id,
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get_containers_for_client(
    pool: &SqlitePool,
    client_id: &str,
    include_stopped: bool,
) -> Result<Vec<Container>> {
    let rows = sqlx::query!(
        r#"SELECT id, client_id, container_name, image, current_digest, latest_digest,
                  update_available as "update_available: i64", update_mode, status, checked_at
           FROM containers WHERE client_id = ?"#,
        client_id
    )
    .fetch_all(pool)
    .await?;

    let containers = rows
        .into_iter()
        .filter(|r| include_stopped || r.status == "running")
        .map(|r| Container {
            id: r.id,
            client_id: r.client_id,
            container_name: r.container_name,
            image: r.image,
            current_digest: r.current_digest,
            latest_digest: r.latest_digest,
            update_available: r.update_available != 0,
            update_mode: r.update_mode,
            status: r.status,
            checked_at: r.checked_at,
        })
        .collect();

    Ok(containers)
}

pub async fn upsert_container(pool: &SqlitePool, container: &Container) -> Result<()> {
    let update_available: i64 = if container.update_available { 1 } else { 0 };
    sqlx::query!(
        r#"INSERT INTO containers (id, client_id, container_name, image, current_digest, latest_digest, update_available, update_mode, status, checked_at)
           VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
           ON CONFLICT(client_id, container_name) DO UPDATE SET
               image = excluded.image,
               status = excluded.status,
               current_digest = COALESCE(excluded.current_digest, current_digest),
               latest_digest = COALESCE(excluded.latest_digest, latest_digest),
               update_available = excluded.update_available,
               checked_at = excluded.checked_at"#,
        container.id,
        container.client_id,
        container.container_name,
        container.image,
        container.current_digest,
        container.latest_digest,
        update_available,
        container.update_mode,
        container.status,
        container.checked_at,
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn update_container_mode(
    pool: &SqlitePool,
    client_id: &str,
    container_name: &str,
    update_mode: &str,
) -> Result<()> {
    sqlx::query!(
        "UPDATE containers SET update_mode = ? WHERE client_id = ? AND container_name = ?",
        update_mode,
        client_id,
        container_name,
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn update_container_digest(
    pool: &SqlitePool,
    client_id: &str,
    container_name: &str,
    current_digest: &str,
    latest_digest: &str,
    update_available: bool,
) -> Result<()> {
    let update_available_int: i64 = if update_available { 1 } else { 0 };
    let now = chrono::Utc::now().to_rfc3339();
    sqlx::query!(
        r#"UPDATE containers SET current_digest = ?, latest_digest = ?, update_available = ?, checked_at = ?
           WHERE client_id = ? AND container_name = ?"#,
        current_digest,
        latest_digest,
        update_available_int,
        now,
        client_id,
        container_name,
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn insert_update_job(pool: &SqlitePool, job: &UpdateJob) -> Result<()> {
    sqlx::query!(
        r#"INSERT INTO update_jobs (id, client_id, container_name, image, from_digest, to_digest, status, output, started_at, completed_at)
           VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
        job.id,
        job.client_id,
        job.container_name,
        job.image,
        job.from_digest,
        job.to_digest,
        job.status,
        job.output,
        job.started_at,
        job.completed_at,
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get_jobs_for_client(pool: &SqlitePool, client_id: &str) -> Result<Vec<UpdateJob>> {
    let rows = sqlx::query!(
        "SELECT id, client_id, container_name, image, from_digest, to_digest, status, output, started_at, completed_at
         FROM update_jobs WHERE client_id = ? ORDER BY started_at DESC",
        client_id
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| UpdateJob {
            id: r.id,
            client_id: r.client_id,
            container_name: r.container_name,
            image: r.image,
            from_digest: r.from_digest,
            to_digest: r.to_digest,
            status: r.status,
            output: r.output,
            started_at: r.started_at,
            completed_at: r.completed_at,
        })
        .collect())
}

pub async fn get_job(pool: &SqlitePool, id: &str) -> Result<Option<UpdateJob>> {
    let row = sqlx::query!(
        "SELECT id, client_id, container_name, image, from_digest, to_digest, status, output, started_at, completed_at
         FROM update_jobs WHERE id = ?",
        id
    )
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| UpdateJob {
        id: r.id,
        client_id: r.client_id,
        container_name: r.container_name,
        image: r.image,
        from_digest: r.from_digest,
        to_digest: r.to_digest,
        status: r.status,
        output: r.output,
        started_at: r.started_at,
        completed_at: r.completed_at,
    }))
}

pub async fn update_job_status(
    pool: &SqlitePool,
    id: &str,
    status: &str,
    output: Option<&str>,
    completed_at: Option<&str>,
) -> Result<()> {
    sqlx::query!(
        "UPDATE update_jobs SET status = ?, output = ?, completed_at = ? WHERE id = ?",
        status,
        output,
        completed_at,
        id,
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn append_job_output(pool: &SqlitePool, id: &str, chunk: &str) -> Result<()> {
    sqlx::query!(
        "UPDATE update_jobs SET output = COALESCE(output, '') || ? WHERE id = ?",
        chunk,
        id,
    )
    .execute(pool)
    .await?;
    Ok(())
}

#[allow(dead_code)]
pub async fn get_recent_jobs(
    pool: &SqlitePool,
    limit: i64,
    offset: i64,
) -> Result<Vec<UpdateJob>> {
    let rows = sqlx::query!(
        "SELECT id, client_id, container_name, image, from_digest, to_digest, status, output, started_at, completed_at
         FROM update_jobs ORDER BY started_at DESC LIMIT ? OFFSET ?",
        limit,
        offset
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| UpdateJob {
            id: r.id,
            client_id: r.client_id,
            container_name: r.container_name,
            image: r.image,
            from_digest: r.from_digest,
            to_digest: r.to_digest,
            status: r.status,
            output: r.output,
            started_at: r.started_at,
            completed_at: r.completed_at,
        })
        .collect())
}

pub async fn get_recent_jobs_filtered(
    pool: &SqlitePool,
    client_id: Option<&str>,
    status: Option<&str>,
    limit: i64,
    offset: i64,
) -> Result<Vec<UpdateJob>> {
    // Build query dynamically based on optional filters
    let rows = match (client_id, status) {
        (Some(cid), Some(st)) => {
            sqlx::query!(
                "SELECT id, client_id, container_name, image, from_digest, to_digest, status, output, started_at, completed_at
                 FROM update_jobs WHERE client_id = ? AND status = ? ORDER BY started_at DESC LIMIT ? OFFSET ?",
                cid, st, limit, offset
            )
            .fetch_all(pool)
            .await?
        }
        (Some(cid), None) => {
            sqlx::query!(
                "SELECT id, client_id, container_name, image, from_digest, to_digest, status, output, started_at, completed_at
                 FROM update_jobs WHERE client_id = ? ORDER BY started_at DESC LIMIT ? OFFSET ?",
                cid, limit, offset
            )
            .fetch_all(pool)
            .await?
        }
        (None, Some(st)) => {
            sqlx::query!(
                "SELECT id, client_id, container_name, image, from_digest, to_digest, status, output, started_at, completed_at
                 FROM update_jobs WHERE status = ? ORDER BY started_at DESC LIMIT ? OFFSET ?",
                st, limit, offset
            )
            .fetch_all(pool)
            .await?
        }
        (None, None) => {
            sqlx::query!(
                "SELECT id, client_id, container_name, image, from_digest, to_digest, status, output, started_at, completed_at
                 FROM update_jobs ORDER BY started_at DESC LIMIT ? OFFSET ?",
                limit, offset
            )
            .fetch_all(pool)
            .await?
        }
    };

    Ok(rows
        .into_iter()
        .map(|r| UpdateJob {
            id: r.id,
            client_id: r.client_id,
            container_name: r.container_name,
            image: r.image,
            from_digest: r.from_digest,
            to_digest: r.to_digest,
            status: r.status,
            output: r.output,
            started_at: r.started_at,
            completed_at: r.completed_at,
        })
        .collect())
}
