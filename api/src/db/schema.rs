use anyhow::Result;
use sqlx::{Row, SqlitePool};

use crate::models::{Client, Container, UpdateJob};

pub async fn get_all_clients(pool: &SqlitePool) -> Result<Vec<Client>> {
    let rows = sqlx::query_as::<_, Client>(
        "SELECT id, name, host, color, compose_file_path, agent_version, agent_update_mode,
                last_seen, connected, created_at FROM clients ORDER BY created_at DESC",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn get_client(pool: &SqlitePool, id: &str) -> Result<Option<Client>> {
    let row = sqlx::query_as::<_, Client>(
        "SELECT id, name, host, color, compose_file_path, agent_version, agent_update_mode,
                last_seen, connected, created_at FROM clients WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn insert_client(pool: &SqlitePool, client: &Client) -> Result<()> {
    let connected: i64 = if client.connected { 1 } else { 0 };
    sqlx::query(
        "INSERT INTO clients (id, name, host, color, compose_file_path, agent_version, agent_update_mode, last_seen, connected, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&client.id)
    .bind(&client.name)
    .bind(&client.host)
    .bind(&client.color)
    .bind(&client.compose_file_path)
    .bind(&client.agent_version)
    .bind(&client.agent_update_mode)
    .bind(&client.last_seen)
    .bind(connected)
    .bind(&client.created_at)
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
    sqlx::query(
        "UPDATE clients SET name = ?, color = ?, compose_file_path = ?, agent_update_mode = ? WHERE id = ?",
    )
    .bind(name)
    .bind(color)
    .bind(compose_file_path)
    .bind(agent_update_mode)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn delete_client(pool: &SqlitePool, id: &str) -> Result<()> {
    sqlx::query("DELETE FROM clients WHERE id = ?")
        .bind(id)
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
    sqlx::query("UPDATE clients SET connected = ?, last_seen = ? WHERE id = ?")
        .bind(connected_int)
        .bind(last_seen)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn set_client_agent_version(pool: &SqlitePool, id: &str, version: &str) -> Result<()> {
    sqlx::query("UPDATE clients SET agent_version = ? WHERE id = ?")
        .bind(version)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn get_containers_for_client(
    pool: &SqlitePool,
    client_id: &str,
    include_stopped: bool,
) -> Result<Vec<Container>> {
    let rows = sqlx::query_as::<_, Container>(
        "SELECT id, client_id, container_name, image, current_digest, latest_digest,
                update_available, update_mode, status, checked_at
         FROM containers WHERE client_id = ?",
    )
    .bind(client_id)
    .fetch_all(pool)
    .await?;

    let containers = rows
        .into_iter()
        .filter(|c| include_stopped || c.status == "running")
        .collect();

    Ok(containers)
}

pub async fn upsert_container(pool: &SqlitePool, container: &Container) -> Result<()> {
    let update_available: i64 = if container.update_available { 1 } else { 0 };
    sqlx::query(
        "INSERT INTO containers (id, client_id, container_name, image, current_digest, latest_digest, update_available, update_mode, status, checked_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(client_id, container_name) DO UPDATE SET
             image = excluded.image,
             status = excluded.status,
             current_digest = COALESCE(excluded.current_digest, current_digest),
             latest_digest = COALESCE(excluded.latest_digest, latest_digest),
             update_available = excluded.update_available,
             checked_at = excluded.checked_at",
    )
    .bind(&container.id)
    .bind(&container.client_id)
    .bind(&container.container_name)
    .bind(&container.image)
    .bind(&container.current_digest)
    .bind(&container.latest_digest)
    .bind(update_available)
    .bind(&container.update_mode)
    .bind(&container.status)
    .bind(&container.checked_at)
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
    sqlx::query(
        "UPDATE containers SET update_mode = ? WHERE client_id = ? AND container_name = ?",
    )
    .bind(update_mode)
    .bind(client_id)
    .bind(container_name)
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
    sqlx::query(
        "UPDATE containers SET current_digest = ?, latest_digest = ?, update_available = ?, checked_at = ?
         WHERE client_id = ? AND container_name = ?",
    )
    .bind(current_digest)
    .bind(latest_digest)
    .bind(update_available_int)
    .bind(&now)
    .bind(client_id)
    .bind(container_name)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn insert_update_job(pool: &SqlitePool, job: &UpdateJob) -> Result<()> {
    sqlx::query(
        "INSERT INTO update_jobs (id, client_id, container_name, image, from_digest, to_digest, status, output, started_at, completed_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&job.id)
    .bind(&job.client_id)
    .bind(&job.container_name)
    .bind(&job.image)
    .bind(&job.from_digest)
    .bind(&job.to_digest)
    .bind(&job.status)
    .bind(&job.output)
    .bind(&job.started_at)
    .bind(&job.completed_at)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get_jobs_for_client(pool: &SqlitePool, client_id: &str) -> Result<Vec<UpdateJob>> {
    let rows = sqlx::query_as::<_, UpdateJob>(
        "SELECT id, client_id, container_name, image, from_digest, to_digest, status, output, started_at, completed_at
         FROM update_jobs WHERE client_id = ? ORDER BY started_at DESC",
    )
    .bind(client_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn get_job(pool: &SqlitePool, id: &str) -> Result<Option<UpdateJob>> {
    let row = sqlx::query_as::<_, UpdateJob>(
        "SELECT id, client_id, container_name, image, from_digest, to_digest, status, output, started_at, completed_at
         FROM update_jobs WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn update_job_status(
    pool: &SqlitePool,
    id: &str,
    status: &str,
    output: Option<&str>,
    completed_at: Option<&str>,
) -> Result<()> {
    sqlx::query(
        "UPDATE update_jobs SET status = ?, output = ?, completed_at = ? WHERE id = ?",
    )
    .bind(status)
    .bind(output)
    .bind(completed_at)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn append_job_output(pool: &SqlitePool, id: &str, chunk: &str) -> Result<()> {
    sqlx::query(
        "UPDATE update_jobs SET output = COALESCE(output, '') || ? WHERE id = ?",
    )
    .bind(chunk)
    .bind(id)
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
    let rows = sqlx::query_as::<_, UpdateJob>(
        "SELECT id, client_id, container_name, image, from_digest, to_digest, status, output, started_at, completed_at
         FROM update_jobs ORDER BY started_at DESC LIMIT ? OFFSET ?",
    )
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn get_recent_jobs_filtered(
    pool: &SqlitePool,
    client_id: Option<&str>,
    status: Option<&str>,
    limit: i64,
    offset: i64,
) -> Result<Vec<UpdateJob>> {
    let rows = match (client_id, status) {
        (Some(cid), Some(st)) => {
            sqlx::query_as::<_, UpdateJob>(
                "SELECT id, client_id, container_name, image, from_digest, to_digest, status, output, started_at, completed_at
                 FROM update_jobs WHERE client_id = ? AND status = ? ORDER BY started_at DESC LIMIT ? OFFSET ?",
            )
            .bind(cid)
            .bind(st)
            .bind(limit)
            .bind(offset)
            .fetch_all(pool)
            .await?
        }
        (Some(cid), None) => {
            sqlx::query_as::<_, UpdateJob>(
                "SELECT id, client_id, container_name, image, from_digest, to_digest, status, output, started_at, completed_at
                 FROM update_jobs WHERE client_id = ? ORDER BY started_at DESC LIMIT ? OFFSET ?",
            )
            .bind(cid)
            .bind(limit)
            .bind(offset)
            .fetch_all(pool)
            .await?
        }
        (None, Some(st)) => {
            sqlx::query_as::<_, UpdateJob>(
                "SELECT id, client_id, container_name, image, from_digest, to_digest, status, output, started_at, completed_at
                 FROM update_jobs WHERE status = ? ORDER BY started_at DESC LIMIT ? OFFSET ?",
            )
            .bind(st)
            .bind(limit)
            .bind(offset)
            .fetch_all(pool)
            .await?
        }
        (None, None) => {
            sqlx::query_as::<_, UpdateJob>(
                "SELECT id, client_id, container_name, image, from_digest, to_digest, status, output, started_at, completed_at
                 FROM update_jobs ORDER BY started_at DESC LIMIT ? OFFSET ?",
            )
            .bind(limit)
            .bind(offset)
            .fetch_all(pool)
            .await?
        }
    };
    Ok(rows)
}

pub async fn get_update_count_for_client(pool: &SqlitePool, client_id: &str) -> Result<i64, sqlx::Error> {
    let row = sqlx::query(
        "SELECT COUNT(*) as count FROM containers WHERE client_id = ? AND update_available = 1",
    )
    .bind(client_id)
    .fetch_one(pool)
    .await?;
    Ok(row.get::<i64, _>("count"))
}
