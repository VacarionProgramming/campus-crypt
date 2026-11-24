// Campus Crypt daily board. The Worker is not deployed yet: leave this empty
// and every call fails open ("Board offline - your run still counts on your share card").
// When Dey deploys it, set the origin only, with no trailing path:
//   var DAILY_API_BASE = "https://<worker-host>";
// No API key belongs in this file. The score checksum is computed in the wasm.
var DAILY_API_BASE = "";

var ccJobs = {};
var ccJobSeq = 1;
var ccName = { open: false, submitted: false };

function ccRead(ptr, len) {
    if (!len || typeof wasm_memory === "undefined" || !wasm_memory) return "";
    var bytes = new Uint8Array(wasm_memory.buffer, ptr, len);
    return new TextDecoder("utf-8").decode(bytes);
}

function ccWrite(ptr, cap, text) {
    var bytes = new TextEncoder().encode(text || "");
    var n = Math.min(bytes.length, cap);
    if (n > 0 && typeof wasm_memory !== "undefined" && wasm_memory) {
        new Uint8Array(wasm_memory.buffer, ptr, cap).set(bytes.subarray(0, n));
    }
    return n;
}

// Fetch one daily endpoint. An empty base rejects at once (fail open).
// Resolves { status, body } for any HTTP response, including 4xx and 5xx.
// Rejects on network failure, abort, or a missing base.
function dailyFetch(base, path, options) {
    options = options || {};
    if (!base) return Promise.reject(new Error("offline"));
    var url = String(base).replace(/\/$/, "") + path;
    var timeout = options.timeoutMs || 5000;
    var ctrl = typeof AbortController !== "undefined" ? new AbortController() : null;
    var timer = setTimeout(function () {
        if (ctrl) ctrl.abort();
    }, timeout);
    var init = {
        method: options.method || "GET",
        headers: { Accept: "application/json" },
    };
    if (options.body) {
        init.headers["Content-Type"] = "application/json";
        init.body = options.body;
    }
    if (ctrl) init.signal = ctrl.signal;
    return fetch(url, init).then(function (r) {
        return r.text().then(function (text) {
            return { status: r.status, body: text || "" };
        });
    }).finally(function () {
        clearTimeout(timer);
    });
}

function ccStart(method, path, body, timeoutMs) {
    var id = ccJobSeq++;
    if (!DAILY_API_BASE) {
        ccJobs[id] = { state: "error", status: 0, body: "" };
        return id;
    }
    ccJobs[id] = { state: "pending", status: 0, body: "" };
    dailyFetch(DAILY_API_BASE, path, {
        method: method === 1 ? "POST" : "GET",
        body: body || "",
        timeoutMs: timeoutMs,
    }).then(function (res) {
        var job = ccJobs[id];
        if (!job || job.state === "taken") return;
        job.state = "done";
        job.status = res.status;
        job.body = res.body || "";
    }).catch(function () {
        var job = ccJobs[id];
        if (!job || job.state === "taken") return;
        job.state = "error";
        job.status = 0;
        job.body = "";
    });
    return id;
}

function ccTake(id, buf, cap) {
    var job = ccJobs[id];
    if (!job || job.state === "pending") return { code: 0, len: 0 };
    delete ccJobs[id];
    if (job.state === "error") return { code: -1, len: 0 };
    return { code: job.status || -1, len: ccWrite(buf, cap, job.body) };
}

if (typeof miniquad_add_plugin === "function") {
    miniquad_add_plugin({
        register_plugin: function (importObject) {
            importObject.env.cc_api_start = function (method, pathPtr, pathLen, bodyPtr, bodyLen, timeoutMs) {
                return ccStart(method, ccRead(pathPtr, pathLen), bodyLen ? ccRead(bodyPtr, bodyLen) : "", timeoutMs);
            };
            importObject.env.cc_api_poll = function (id, buf, cap, lenPtr) {
                var got = ccTake(id, buf, cap);
                if (lenPtr && typeof wasm_memory !== "undefined" && wasm_memory) {
                    new DataView(wasm_memory.buffer).setUint32(lenPtr, got.len, true);
                }
                return got.code;
            };
            importObject.env.cc_api_forget = function (id) {
                var job = ccJobs[id];
                if (job) job.state = "taken";
                delete ccJobs[id];
            };
            importObject.env.cc_store_get = function (keyPtr, keyLen, buf, cap) {
                try {
                    var v = window.localStorage.getItem(ccRead(keyPtr, keyLen));
                    if (v === null) return -1;
                    return ccWrite(buf, cap, v);
                } catch (e) {
                    return -1;
                }
            };
            importObject.env.cc_store_set = function (keyPtr, keyLen, valPtr, valLen) {
                try {
                    window.localStorage.setItem(ccRead(keyPtr, keyLen), ccRead(valPtr, valLen));
                } catch (e) { }
            };
            importObject.env.cc_daily_query = function (buf, cap) {
                try {
                    var q = new URLSearchParams(window.location.search).get("daily") || "";
                    if (!q) return 0;
                    return ccWrite(buf, cap, q);
                } catch (e) {
                    return 0;
                }
            };
            importObject.env.cc_clipboard = function (ptr, len) {
                var text = ccRead(ptr, len);
                var ok = 0;
                try {
                    if (navigator.clipboard && navigator.clipboard.writeText) {
                        navigator.clipboard.writeText(text).catch(function () { });
                        ok = 1;
                    }
                } catch (e) { }
                try {
                    var ta = document.createElement("textarea");
                    ta.value = text;
                    ta.setAttribute("readonly", "");
                    ta.style.position = "fixed";
                    ta.style.left = "-9999px";
                    document.body.appendChild(ta);
                    ta.select();
                    if (document.execCommand("copy")) ok = 1;
                    document.body.removeChild(ta);
                } catch (e) { }
                return ok;
            };
            importObject.env.cc_name_open = function (nickPtr, nickLen, clubPtr, clubLen, errPtr, errLen) {
                var box = document.getElementById("daily-name");
                if (!box) return;
                document.getElementById("daily-nick").value = ccRead(nickPtr, nickLen);
                document.getElementById("daily-club").value = ccRead(clubPtr, clubLen);
                document.getElementById("daily-name-err").textContent = ccRead(errPtr, errLen);
                box.style.display = "flex";
                ccName.open = true;
                ccName.submitted = false;
                document.getElementById("daily-nick").focus();
            };
            importObject.env.cc_name_close = function () {
                var box = document.getElementById("daily-name");
                if (box) box.style.display = "none";
                ccName.open = false;
            };
            importObject.env.cc_name_poll = function (nickBuf, nickCap, nickLenPtr, clubBuf, clubCap, clubLenPtr) {
                if (!ccName.submitted) return 0;
                ccName.submitted = false;
                var nick = document.getElementById("daily-nick").value || "";
                var club = document.getElementById("daily-club").value || "";
                var n = ccWrite(nickBuf, nickCap, nick);
                var c = ccWrite(clubBuf, clubCap, club);
                if (typeof wasm_memory !== "undefined" && wasm_memory) {
                    var view = new DataView(wasm_memory.buffer);
                    view.setUint32(nickLenPtr, n, true);
                    view.setUint32(clubLenPtr, c, true);
                }
                return 1;
            };
        },
        name: "cc_daily",
        version: 1,
    });
}

if (typeof document !== "undefined") {
    var form = document.getElementById("daily-name-form");
    if (form) {
        form.addEventListener("submit", function (ev) {
            ev.preventDefault();
            ccName.submitted = true;
            var box = document.getElementById("daily-name");
            if (box) box.style.display = "none";
        });
    }
}

if (typeof module !== "undefined" && module.exports) {
    module.exports = {
        dailyFetch: dailyFetch,
        setBase: function (b) { DAILY_API_BASE = b; },
        getBase: function () { return DAILY_API_BASE; },
    };
}
