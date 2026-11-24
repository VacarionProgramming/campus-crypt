// Tiny check of web/api.js against a stub Worker. Run: node web/api.test.js
const http = require("http");
const api = require("./api.js");

function listen(handler) {
    return new Promise((resolve) => {
        const server = http.createServer(handler);
        server.listen(0, "127.0.0.1", () => resolve(server));
    });
}

async function main() {
    let posts = 0;
    const server = await listen((req, res) => {
        const url = req.url || "";
        if (req.method === "GET" && url === "/v1/daily/today") {
            res.writeHead(200, { "Content-Type": "application/json" });
            res.end(JSON.stringify({ api: 1, date: "2026-11-24", seed_hex: "b4a84055cea0c3d1", tz: "America/New_York" }));
            return;
        }
        if (req.method === "POST" && url === "/v1/daily/2026-11-24/scores") {
            posts += 1;
            let body = "";
            req.on("data", (c) => { body += c; });
            req.on("end", () => {
                const json = JSON.parse(body);
                if (json.mode !== "daily" || json.nickname !== "Hoot_Fan") {
                    res.writeHead(400, { "Content-Type": "application/json" });
                    res.end(JSON.stringify({ api: 1, error: { code: "bad_nickname", message: "no" } }));
                    return;
                }
                res.writeHead(201, { "Content-Type": "application/json" });
                res.end(JSON.stringify({ api: 1, accepted: true, date: "2026-11-24", rank: 4, players: 37 }));
            });
            return;
        }
        if (req.method === "GET" && url.startsWith("/v1/daily/2026-11-24/leaderboard")) {
            res.writeHead(200, { "Content-Type": "application/json" });
            res.end(JSON.stringify({
                api: 1, date: "2026-11-24", seed_hex: "b4a84055cea0c3d1", players: 1,
                rows: [{ rank: 1, nickname: "Hoot_Fan", club: "CSE", floors_reached: 7, floor_count: 10, time_ms: 612345, glyphs: "CCMCCCX..." }],
                clubs: [{ club: "CSE", players: 6, best_floors: 10, avg_floors: 7.5 }],
            }));
            return;
        }
        res.writeHead(500);
        res.end("nope");
    });
    const base = "http://127.0.0.1:" + server.address().port;
    api.setBase("");
    let offline = false;
    try {
        await api.dailyFetch(api.getBase(), "/v1/daily/today", { timeoutMs: 200 });
    } catch (e) {
        offline = true;
    }
    if (!offline) throw new Error("empty base should fail open");

    api.setBase(base);
    const today = await api.dailyFetch(base, "/v1/daily/today", { timeoutMs: 2000 });
    if (today.status !== 200 || !today.body.includes("b4a84055cea0c3d1")) throw new Error("today: " + today.status);

    const score = await api.dailyFetch(base, "/v1/daily/2026-11-24/scores", {
        method: "POST",
        body: JSON.stringify({ mode: "daily", nickname: "Hoot_Fan", club: "CSE" }),
        timeoutMs: 2000,
    });
    if (score.status !== 201 || !score.body.includes('"rank":4')) throw new Error("score: " + score.body);
    if (posts !== 1) throw new Error("expected one post");

    const board = await api.dailyFetch(base, "/v1/daily/2026-11-24/leaderboard?limit=20", { timeoutMs: 2000 });
    if (board.status !== 200 || !board.body.includes("CSE")) throw new Error("board");

    let down = false;
    try {
        await api.dailyFetch("http://127.0.0.1:1", "/v1/daily/today", { timeoutMs: 500 });
    } catch (e) {
        down = true;
    }
    if (!down) throw new Error("a dead port should reject");

    server.close();
    console.log("api.js: 5 checks ok");
}

main().catch((e) => {
    console.error(e);
    process.exit(1);
});
