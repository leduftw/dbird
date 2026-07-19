const USERNAME_PATTERN = /^[A-Za-z0-9][A-Za-z0-9_-]{2,15}$/;
const USERNAME_KEY_PATTERN = /^[a-z0-9][a-z0-9_-]{2,15}$/;
const CREDENTIAL_PATTERN = /^[a-f0-9]{64}$/;
const MAX_SCORE = 4_294_967_295;
const MAX_BODY_BYTES = 4_096;
const LEADERBOARD_LIMIT = 10;

const JSON_HEADERS = {
  "content-type": "application/json; charset=utf-8",
  "cache-control": "no-store",
  "x-content-type-options": "nosniff",
};

export default {
  async fetch(request, environment) {
    if (!environment.DB) {
      return json({ error: "Leaderboard database is not configured." }, 503);
    }

    try {
      return await handleRequest(request, new D1LeaderboardStore(environment.DB));
    } catch (error) {
      console.error("Unhandled leaderboard error", error);
      return json({ error: "Leaderboard service is temporarily unavailable." }, 500);
    }
  },
};

export async function handleRequest(request, store) {
  const url = new URL(request.url);

  if (request.method === "GET" && url.pathname === "/health") {
    return json({ ok: true });
  }

  if (request.method === "GET" && url.pathname === "/v1/leaderboard") {
    const leaderboard = await store.leaderboard();
    return json(
      { leaderboard },
      200,
      { "cache-control": "public, max-age=15" },
    );
  }

  if (request.method === "POST" && url.pathname === "/v1/players") {
    const body = await readJson(request);
    if (body instanceof Response) {
      return body;
    }
    const validation = validateRegistration(body);
    if (validation instanceof Response) {
      return validation;
    }

    const credentialHash = await hashCredential(validation.credential);
    const registered = await store.register(
      validation.usernameKey,
      validation.username,
      credentialHash,
    );
    if (!registered) {
      return json(
        { error: "That username is already claimed on another installation." },
        409,
      );
    }
    return json(await store.snapshot(validation.usernameKey));
  }

  const scoreMatch = url.pathname.match(
    /^\/v1\/players\/([A-Za-z0-9][A-Za-z0-9_-]{2,15})\/score$/,
  );
  if (request.method === "PUT" && scoreMatch) {
    const usernameKey = scoreMatch[1].toLowerCase();
    if (!USERNAME_KEY_PATTERN.test(usernameKey)) {
      return json({ error: "Invalid username." }, 400);
    }
    const credential = bearerCredential(request.headers.get("authorization"));
    if (!credential) {
      return json({ error: "A valid player credential is required." }, 401);
    }
    const body = await readJson(request);
    if (body instanceof Response) {
      return body;
    }
    if (
      typeof body.score !== "number" ||
      !Number.isSafeInteger(body.score) ||
      body.score < 0 ||
      body.score > MAX_SCORE
    ) {
      return json(
        { error: `Score must be an integer between 0 and ${MAX_SCORE}.` },
        400,
      );
    }

    const credentialHash = await hashCredential(credential);
    const updated = await store.updateScore(
      usernameKey,
      credentialHash,
      body.score,
    );
    if (!updated) {
      return json({ error: "Player credential was not accepted." }, 401);
    }
    return json(await store.snapshot(usernameKey));
  }

  return json({ error: "Not found." }, 404);
}

export class D1LeaderboardStore {
  constructor(database) {
    this.database = database;
  }

  async register(usernameKey, displayUsername, credentialHash) {
    await this.database
      .prepare(
        `INSERT INTO players (
           username_key, display_username, credential_hash,
           high_score, created_at, achieved_at
         ) VALUES (?1, ?2, ?3, 0, unixepoch(), NULL)
         ON CONFLICT(username_key) DO NOTHING`,
      )
      .bind(usernameKey, displayUsername, credentialHash)
      .run();

    const player = await this.database
      .prepare(
        `SELECT credential_hash
           FROM players
          WHERE username_key = ?1`,
      )
      .bind(usernameKey)
      .first();
    return player?.credential_hash === credentialHash;
  }

  async updateScore(usernameKey, credentialHash, score) {
    const player = await this.database
      .prepare(
        `UPDATE players
            SET high_score = max(high_score, ?1),
                achieved_at = CASE
                  WHEN ?1 > high_score THEN unixepoch()
                  ELSE achieved_at
                END
          WHERE username_key = ?2
            AND credential_hash = ?3
        RETURNING username_key`,
      )
      .bind(score, usernameKey, credentialHash)
      .first();
    return Boolean(player);
  }

  async leaderboard() {
    const result = await this.database
      .prepare(
        `SELECT display_username AS username, high_score AS score
           FROM players
          WHERE high_score > 0
          ORDER BY high_score DESC, achieved_at ASC, username_key ASC
          LIMIT ?1`,
      )
      .bind(LEADERBOARD_LIMIT)
      .all();
    return result.results.map((entry, index) => ({
      rank: index + 1,
      username: entry.username,
      score: entry.score,
    }));
  }

  async snapshot(usernameKey) {
    const stored = await this.database
      .prepare(
        `SELECT display_username AS username,
                high_score AS score,
                achieved_at
           FROM players
          WHERE username_key = ?1`,
      )
      .bind(usernameKey)
      .first();
    const leaderboard = await this.leaderboard();
    const visible = leaderboard.find((entry) =>
      entry.username.toLowerCase() === usernameKey
    );
    if (visible) {
      return {
        player: {
          username: stored.username,
          score: stored.score,
          rank: visible.rank,
        },
        leaderboard,
      };
    }
    let rank = null;
    if (stored.score > 0) {
      const result = await this.database
        .prepare(
          `SELECT count(*) AS players_ahead
             FROM players
            WHERE high_score > ?1
               OR (high_score = ?1 AND achieved_at < ?2)
               OR (high_score = ?1 AND achieved_at = ?2 AND username_key < ?3)`,
        )
        .bind(stored.score, stored.achieved_at, usernameKey)
        .first();
      rank = Number(result.players_ahead) + 1;
    }
    return {
      player: { username: stored.username, score: stored.score, rank },
      leaderboard,
    };
  }
}

function validateRegistration(body) {
  if (
    body === null ||
    typeof body !== "object" ||
    typeof body.username !== "string" ||
    !USERNAME_PATTERN.test(body.username)
  ) {
    return json(
      {
        error:
          "Username must be 3-16 ASCII letters, numbers, `_`, or `-`, and start with a letter or number.",
      },
      400,
    );
  }
  if (
    typeof body.credential !== "string" ||
    !CREDENTIAL_PATTERN.test(body.credential)
  ) {
    return json({ error: "Invalid player credential." }, 400);
  }
  return {
    username: body.username,
    usernameKey: body.username.toLowerCase(),
    credential: body.credential,
  };
}

async function readJson(request) {
  const contentLength = Number(request.headers.get("content-length") || "0");
  if (Number.isFinite(contentLength) && contentLength > MAX_BODY_BYTES) {
    return json({ error: "Request body is too large." }, 413);
  }
  if (!request.body) {
    return json({ error: "Request body must be valid JSON." }, 400);
  }

  try {
    const reader = request.body.getReader();
    const chunks = [];
    let totalBytes = 0;
    while (true) {
      const { done, value } = await reader.read();
      if (done) {
        break;
      }
      totalBytes += value.byteLength;
      if (totalBytes > MAX_BODY_BYTES) {
        await reader.cancel();
        return json({ error: "Request body is too large." }, 413);
      }
      chunks.push(value);
    }
    const document = new Uint8Array(totalBytes);
    let offset = 0;
    for (const chunk of chunks) {
      document.set(chunk, offset);
      offset += chunk.byteLength;
    }
    const body = JSON.parse(new TextDecoder().decode(document));
    if (body === null || typeof body !== "object" || Array.isArray(body)) {
      return json({ error: "Request body must be a JSON object." }, 400);
    }
    return body;
  } catch {
    return json({ error: "Request body must be valid JSON." }, 400);
  }
}

function bearerCredential(header) {
  const match = header?.match(/^Bearer ([a-f0-9]{64})$/);
  return match?.[1] ?? null;
}

async function hashCredential(credential) {
  const bytes = new TextEncoder().encode(credential);
  const digest = await crypto.subtle.digest("SHA-256", bytes);
  return [...new Uint8Array(digest)]
    .map((byte) => byte.toString(16).padStart(2, "0"))
    .join("");
}

function json(body, status = 200, extraHeaders = {}) {
  return new Response(JSON.stringify(body), {
    status,
    headers: { ...JSON_HEADERS, ...extraHeaders },
  });
}
