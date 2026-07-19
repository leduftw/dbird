import assert from "node:assert/strict";
import { test } from "node:test";

import { handleRequest } from "../src/index.js";

const ALPHA_CREDENTIAL = "a".repeat(64);
const BETA_CREDENTIAL = "b".repeat(64);

class MemoryStore {
  constructor() {
    this.players = new Map();
    this.clock = 0;
  }

  async register(key, username, credentialHash) {
    const existing = this.players.get(key);
    if (existing) {
      return existing.credentialHash === credentialHash;
    }
    this.players.set(key, {
      key,
      username,
      credentialHash,
      score: 0,
      achievedAt: null,
    });
    return true;
  }

  async updateScore(key, credentialHash, score) {
    const player = this.players.get(key);
    if (!player || player.credentialHash !== credentialHash) {
      return false;
    }
    if (score > player.score) {
      player.score = score;
      player.achievedAt = ++this.clock;
    }
    return true;
  }

  ranked() {
    return [...this.players.values()]
      .filter((player) => player.score > 0)
      .sort(
        (left, right) =>
          right.score - left.score ||
          left.achievedAt - right.achievedAt ||
          left.key.localeCompare(right.key),
      )
      .map((player, index) => ({ ...player, rank: index + 1 }));
  }

  async leaderboard() {
    return this.ranked()
      .slice(0, 10)
      .map(({ rank, username, score }) => ({ rank, username, score }));
  }

  async snapshot(key) {
    const stored = this.players.get(key);
    const ranked = this.ranked().find((player) => player.key === key);
    return {
      player: {
        username: stored.username,
        score: stored.score,
        rank: ranked?.rank ?? null,
      },
      leaderboard: await this.leaderboard(),
    };
  }
}

function request(path, { method = "GET", body, credential } = {}) {
  const headers = {};
  if (body !== undefined) {
    headers["content-type"] = "application/json";
  }
  if (credential) {
    headers.authorization = `Bearer ${credential}`;
  }
  return new Request(`https://leaderboard.example${path}`, {
    method,
    headers,
    body: body === undefined ? undefined : JSON.stringify(body),
  });
}

async function json(response) {
  return { status: response.status, body: await response.json() };
}

async function register(store, username, credential = ALPHA_CREDENTIAL) {
  return json(
    await handleRequest(
      request("/v1/players", {
        method: "POST",
        body: { username, credential },
      }),
      store,
    ),
  );
}

async function submit(store, username, score, credential = ALPHA_CREDENTIAL) {
  return json(
    await handleRequest(
      request(`/v1/players/${username}/score`, {
        method: "PUT",
        body: { score },
        credential,
      }),
      store,
    ),
  );
}

test("health and empty leaderboard are public", async () => {
  const store = new MemoryStore();
  assert.deepEqual(
    await json(await handleRequest(request("/health"), store)),
    { status: 200, body: { ok: true } },
  );
  assert.deepEqual(
    await json(await handleRequest(request("/v1/leaderboard"), store)),
    { status: 200, body: { leaderboard: [] } },
  );
});

test("registration is idempotent for one installation and claims case-insensitively", async () => {
  const store = new MemoryStore();
  assert.equal((await register(store, "BirdOne")).status, 200);
  assert.equal((await register(store, "BirdOne")).status, 200);

  const conflict = await register(store, "birdone", BETA_CREDENTIAL);
  assert.equal(conflict.status, 409);
  assert.match(conflict.body.error, /already claimed/);
});

test("score submission authenticates, only raises the best, and returns ranks", async () => {
  const store = new MemoryStore();
  await register(store, "Alpha", ALPHA_CREDENTIAL);
  await register(store, "Beta", BETA_CREDENTIAL);

  const unauthorized = await submit(store, "Alpha", 99, BETA_CREDENTIAL);
  assert.equal(unauthorized.status, 401);

  assert.equal((await submit(store, "Alpha", 12, ALPHA_CREDENTIAL)).status, 200);
  const beta = await submit(store, "Beta", 20, BETA_CREDENTIAL);
  assert.equal(beta.body.player.rank, 1);
  assert.deepEqual(beta.body.leaderboard, [
    { rank: 1, username: "Beta", score: 20 },
    { rank: 2, username: "Alpha", score: 12 },
  ]);

  const lowerScore = await submit(store, "Alpha", 3, ALPHA_CREDENTIAL);
  assert.equal(lowerScore.body.player.score, 12);
});

test("players outside the top ten still receive their global rank", async () => {
  const store = new MemoryStore();
  let lastResponse;
  for (let index = 0; index < 11; index += 1) {
    const username = `Bird${String(index).padStart(2, "0")}`;
    await register(store, username);
    lastResponse = await submit(store, username, 100 - index);
  }

  assert.equal(lastResponse.body.player.rank, 11);
  assert.equal(lastResponse.body.leaderboard.length, 10);
  assert.equal(lastResponse.body.leaderboard[0].username, "Bird00");
  assert.equal(lastResponse.body.leaderboard[9].username, "Bird09");
});

test("invalid names, credentials, JSON, and scores are rejected", async () => {
  const store = new MemoryStore();
  assert.equal((await register(store, "no spaces")).status, 400);
  assert.equal((await register(store, "Bird", "short")).status, 400);

  const malformed = await handleRequest(
    new Request("https://leaderboard.example/v1/players", {
      method: "POST",
      body: "{",
    }),
    store,
  );
  assert.equal(malformed.status, 400);

  const nullBody = await handleRequest(
    request("/v1/players", { method: "POST", body: null }),
    store,
  );
  assert.equal(nullBody.status, 400);

  const oversized = await handleRequest(
    request("/v1/players", {
      method: "POST",
      body: { padding: "x".repeat(5_000) },
    }),
    store,
  );
  assert.equal(oversized.status, 413);

  await register(store, "Bird");
  for (const score of [-1, 1.5, 4_294_967_296, "10"]) {
    assert.equal((await submit(store, "Bird", score)).status, 400);
  }
});

test("unknown routes return JSON 404 responses", async () => {
  const response = await json(
    await handleRequest(request("/not-a-route"), new MemoryStore()),
  );
  assert.equal(response.status, 404);
  assert.equal(response.body.error, "Not found.");
});
