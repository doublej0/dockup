# DockUp

Self-hosted Docker container management platform with real-time update control, multi-host agent architecture, and update history tracking.

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                        Browser / UI                             │
│                    http://your-server:3100                      │
└───────────────────────────┬─────────────────────────────────────┘
                            │ HTTP / WebSocket
┌───────────────────────────▼─────────────────────────────────────┐
│                      DockUp Web (Astro SSR)                     │
│                         port 3100                               │
└───────────────────────────┬─────────────────────────────────────┘
                            │ HTTP / WebSocket
┌───────────────────────────▼─────────────────────────────────────┐
│                      DockUp API (Rust/Axum)                     │
│                         port 3101                               │
│   ┌───────────────────────────────────────────────────────┐    │
│   │  SQLite DB  │  WebSocket Hub  │  REST Endpoints       │    │
│   └───────────────────────────────────────────────────────┘    │
└────────────┬────────────────────────────────────────────────────┘
             │ WebSocket (JWT auth, persistent)
    ┌────────┴────────┐
    │                 │
┌───▼────┐       ┌────▼───┐
│ Agent  │       │ Agent  │   ... (one per Docker host)
│ Host A │       │ Host B │
│ :2375  │       │ :2375  │
└────────┘       └────────┘
```

## Quick Start

### Prerequisites

- Docker and Docker Compose on your server
- A `.env` file (copy from `.env.example`)

### 1. Clone and configure

```bash
git clone https://github.com/doublej0/dockup.git
cd dockup
cp .env.example .env
# Edit .env — set DOCKUP_PUBLIC_API_URL to your server's IP/hostname and a strong JWT_SECRET
nano .env
```

### 2. Start the stack

```bash
docker compose up -d
```

The UI is available at `http://your-server:3100`.

### 3. Add your first Docker host

Open the UI and click **"Add Client"**. Fill in:

- **Name** — a friendly label for the host
- **Host** — IP or hostname of the remote Docker host
- **SSH User / Password** — credentials used only during onboarding (never stored)
- **Compose file path** — optional path to a `docker-compose.yml` on the remote host
- **Agent update mode** — `manual` or `auto`

DockUp will SSH into the host, install the agent binary via systemd, and connect it automatically. SSH credentials are discarded immediately after installation.

## Agent Installation (automatic)

The onboarding flow:
1. SSH into the target host using the provided credentials
2. Detect CPU architecture (`uname -m`)
3. Write agent config to `/etc/dockup-agent/config.toml`
4. Download the correct `dockup-agent` binary from GitHub Releases
5. Install as a systemd service and start it
6. Wait up to 30 seconds for the agent to establish a WebSocket connection

The agent requires no credentials beyond the JWT token in its config file.

## Environment Variables

| Variable | Required | Description |
|---|---|---|
| `DOCKUP_PUBLIC_API_URL` | Yes | Public URL of the API, used by the web UI and agent config |
| `JWT_SECRET` | Yes | Secret key for signing agent JWT tokens (keep this secret) |
| `DATABASE_URL` | No | SQLite path (default: `sqlite:///data/dockup.db`) |
| `RUST_LOG` | No | Log level for the API (default: `info`) |

## License

MIT
