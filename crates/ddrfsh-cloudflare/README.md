# ddrfsh-cloudflare

A Cloudflare DDNS client. It discovers this machine's public IP and updates the
matching `A` or `AAAA` record through the Cloudflare v4 API.

It is a single-purpose binary: it does not manage zones, create records unless
explicitly asked, or track state across runs.

## Requirements

A Cloudflare API token with **Zone → DNS → Edit** permission for the target
zone. The token only needs access to that one zone.

Find the zone ID on the zone's overview page in the Cloudflare dashboard, or:

```bash
curl -s -H "Authorization: Bearer $CLOUDFLARE_API_TOKEN" \
  "https://api.cloudflare.com/client/v4/zones?name=example.com" |
  jq -r '.result[0].id'
```

## Configuration

All configuration is read from the environment, so secrets never appear in
shell history or process listings.

| Variable               | Required | Description                                                               |
| ---------------------- | -------- | ------------------------------------------------------------------------- |
| `CLOUDFLARE_API_TOKEN` | yes      | API token with `Zone.DNS:Edit`                                            |
| `CLOUDFLARE_ZONE_ID`   | yes      | 32-character hexadecimal zone ID                                          |
| `CLOUDFLARE_RECORD`    | yes      | Full record name, e.g. `home.example.com`; use `example.com` for the apex |

`CLOUDFLARE_RECORD` must be ASCII; pass Punycode for internationalized names.
Trailing dots and wildcards are rejected.

## Usage

Update an existing `A` record and exit:

```bash
CLOUDFLARE_API_TOKEN=... \
CLOUDFLARE_ZONE_ID=... \
CLOUDFLARE_RECORD=home.example.com \
  ddrfsh-cloudflare
```

Update the `AAAA` record instead:

```bash
CLOUDFLARE_API_TOKEN=... CLOUDFLARE_ZONE_ID=... CLOUDFLARE_RECORD=home.example.com \
  ddrfsh-cloudflare --ip-family ipv6
```

Keep the record up to date, polling every 5 minutes:

```bash
CLOUDFLARE_API_TOKEN=... CLOUDFLARE_ZONE_ID=... CLOUDFLARE_RECORD=home.example.com \
  ddrfsh-cloudflare --daemon
```

Create the record if it does not exist yet:

```bash
CLOUDFLARE_API_TOKEN=... CLOUDFLARE_ZONE_ID=... CLOUDFLARE_RECORD=home.example.com \
  ddrfsh-cloudflare --create
```

## Container image

Images are published to `ghcr.io/ddrfsh/cloudflare`:

```bash
docker run --rm \
  -e CLOUDFLARE_API_TOKEN=... \
  -e CLOUDFLARE_ZONE_ID=... \
  -e CLOUDFLARE_RECORD=home.example.com \
  ghcr.io/ddrfsh/cloudflare --ip-family ipv4
```

The image is distroless: it contains only the binary, so run it as a command
container or a long-lived daemon with `--daemon`.

## Options

| Option                     | Default | Description                                                                                   |
| -------------------------- | ------- | --------------------------------------------------------------------------------------------- |
| `--ip-family <ipv4\|ipv6>` | `ipv4`  | Selects both the discovered address family and the record type: `ipv4` → `A`, `ipv6` → `AAAA` |
| `--daemon`                 | off     | Poll continuously instead of running once                                                     |
| `--interval <SECONDS>`     | `300`   | Polling interval; requires `--daemon`, minimum 1                                              |
| `--create`                 | off     | Create the record when no matching record exists                                              |
| `-h, --help`               |         | Show help                                                                                     |
| `-V, --version`            |         | Show version                                                                                  |

## Behaviour

One-shot mode:

1. Discover the public IP for the selected family.
2. Find records matching `CLOUDFLARE_RECORD` and the corresponding type.
3. Update the single match, or create it when `--create` is set.
4. Exit.

Nothing is written to disk.

Daemon mode:

- The two steps above run in a loop; the last successfully updated IP is kept
  **in memory only**. A restart therefore performs one redundant, idempotent
  update.
- If the IP is unchanged, the provider is not called.
- A failed poll is logged and retried on the next interval; it never stops the
  daemon.

Safety rules:

- The record name and type are verified before anything is written.
- More than one matching record is treated as ambiguous: the run fails and no
  record is modified.
- Updates send a partial `PATCH` containing only the IP, so TTL and proxy
  settings are preserved.
- Created records use `ttl: 1` (automatic) and `proxied: false`.

## Exit status

`0` on success, `1` on configuration, discovery, or API errors, and `2` for
invalid command-line usage. Errors are printed to stderr with the environment
variable or endpoint involved; the API token is never printed.
