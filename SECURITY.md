# Security Policy

## Reporting a vulnerability

Please do **not** open a public issue for security problems. Instead, report
privately via GitHub Security Advisories ("Report a vulnerability" on the repo's
Security tab) or by contacting the maintainer directly.

Include: affected version/commit, reproduction steps, and impact. We aim to
acknowledge reports within a few days.

## Handling secrets

SynapzCore never commits credentials. The following are gitignored and must stay local:

- `.env`, `*.env` (except `.env.example`)
- `data/` (Supabase config, goals)
- `*_config.json`, `*.secret.json`

Supabase access uses `SUPABASE_URL` / `SUPABASE_KEY` from the environment (preferred)
or a local `data/supabase_config.json`. Neither is required to build, test, or start
the MCP server — the memory layer degrades to a local on-disk queue when absent.

## Network surface

- The MCP server speaks stdio to a local IDE; it does not open a network port.
- The optional dashboard HTTP API exempts `localhost` and requires `SYNAPZ_API_TOKEN`
  (header `X-Synapz-Token` or `?token=`) for non-localhost requests. When the variable
  is empty, auth is disabled entirely — always set a token before exposing the port or a
  public tunnel.

## Supported versions

Pre-1.0: only the latest commit on `main` is supported.
