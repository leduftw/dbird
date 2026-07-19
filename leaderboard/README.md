# dbird leaderboard service

This directory contains the optional global leaderboard API, implemented as a Cloudflare Worker backed by D1. The Rust client talks to a small HTTP contract, so the backend can be replaced later without changing game or UI code.

Production runs at [`dbird-leaderboard.leduftw.workers.dev`](https://dbird-leaderboard.leduftw.workers.dev). The D1 database ID in `wrangler.jsonc` is public configuration, not a credential; deployment still requires an authorized Cloudflare account.

## Why Cloudflare Workers + D1

This workload is tiny, global, and bursty. Cloudflare keeps the API and database in one deployable project, uses quota-based free limits rather than an inactivity-pause workflow, and currently gives the Workers Free plan 100,000 requests per day. D1's free allowance includes 5 million rows read per day, 100,000 rows written per day, 5 GB total storage, and no D1 egress charge. If the game outgrows that, Workers Paid starts at $5/month and removes the D1 daily caps. See the official [Workers pricing](https://developers.cloudflare.com/workers/platform/pricing/) and [D1 pricing](https://developers.cloudflare.com/d1/platform/pricing/) pages before deployment because limits can change.

Azure is a sound second choice if keeping everything in one account matters more than operational simplicity. Azure Functions Consumption currently includes a monthly execution grant, and one Cosmos DB account per subscription can opt into a lifetime free tier of 1,000 RU/s and 25 GB. Cosmos free tier must be selected when the account is created and is not available for serverless accounts. See [Azure Functions pricing](https://azure.microsoft.com/en-us/pricing/details/functions/) and [Cosmos DB free tier](https://learn.microsoft.com/azure/cosmos-db/free-tier).

## Local development

Install the pinned project dependencies and initialize a local D1 database:

```sh
cd leaderboard
npm ci
npm run db:local
npm run dev
```

In a second terminal, run the game against the local Worker:

```sh
DBIRD_LEADERBOARD_URL=http://127.0.0.1:8787 \
  cargo run -- --mute --online BirdPlayer
```

Run all service checks with:

```sh
npm test
npx wrangler deploy --dry-run
```

## Production deployment

After authenticating with `npx wrangler login`, apply any new migrations and deploy:

```sh
npm run db:remote
npm run deploy
curl https://dbird-leaderboard.leduftw.workers.dev/health
```

For a new Cloudflare account or a fork:

1. Create or sign in to a Cloudflare account, then authenticate Wrangler:

   ```sh
   npx wrangler login
   ```

2. Create the database:

   ```sh
   npx wrangler d1 create dbird-leaderboard
   ```

3. Replace the existing `database_id` in [`wrangler.jsonc`](wrangler.jsonc) with the returned database ID.

4. Apply the schema and deploy:

   ```sh
   npm run db:remote
   npm run deploy
   ```

5. Smoke-test the returned HTTPS URL:

   ```sh
   curl https://YOUR_WORKER.workers.dev/health
   ```

6. Set that base URL as `OFFICIAL_ENDPOINT` in the Rust client. It can also be embedded in a custom release build:

   ```sh
   DBIRD_LEADERBOARD_URL=https://YOUR_WORKER.workers.dev cargo build --release
   ```

The same environment variable can override an embedded URL at runtime, which is useful for staging and local development.

## Data and security model

The service stores a public display username, its normalized lookup key, the best score, timestamps, and a SHA-256 hash of a random per-installation credential. It does not store the raw credential, email address, or password. Usernames are claimed case-insensitively; the local credential lets the same installation update its score without letting another normal client take over the name. There is deliberately no account-recovery flow in this first version, so moving a claimed name to another machine requires securely copying that username's local profile.

Online mode remains usable through outages. Before upload, the client writes a pending best to its private local profile. A successful cloud response clears that queue; a failed request leaves it for a later refresh or online launch.

This is intentionally a casual leaderboard. Authentication prevents ordinary username overwrites, but an open-source client can still fabricate its own score. A competitive leaderboard would need the client to submit a deterministic input replay and the server to re-simulate and validate the run. Rate limiting and abuse monitoring should also be added before promoting the game to a large untrusted audience.

## API

- `GET /health` returns service health.
- `GET /v1/leaderboard` returns the global top ten.
- `POST /v1/players` claims or resumes a username using a client credential.
- `PUT /v1/players/:username/score` raises that player's best and returns the updated leaderboard snapshot.

Scores only move upward. Ties are ordered by the time the score was first achieved, then by normalized username.
