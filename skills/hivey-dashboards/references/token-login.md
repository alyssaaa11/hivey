# Token login for a dashboard (opens logged in from the agent fleet)

The fleet page opens a dashboard as `https://<site>/?token=<the user's dashboard token>` inside a frame.
The site trades the token for its own session cookie and drops the token from the address bar. One token
per user (EnvSave `DASHBOARD_TOKEN_<USER>`, 32+ characters) opens all of that user's dashboards.

## The contract (any stack)

1. **Any page with `?token=`**: redirect (303) to `/api/auth/token?token=<t>&next=<same path and query
   without token>`, with `Referrer-Policy: no-referrer`. Do it in middleware, or first thing in the page's
   script for a static site.
2. **`GET /api/auth/token`**:
   - reject tokens shorter than 32 characters, and any token when the expected one is missing;
   - compare in constant time (hash both sides, then `timingSafeEqual`);
   - wrong → `401 Invalid or expired dashboard token`;
   - right → `303` to `next` (only a path starting with `/`, never `//` or `/\`) with
     `Cache-Control: no-store`, `Referrer-Policy: no-referrer` and
     `Set-Cookie: session=<exp>.<hmac(exp)>; Path=/; Max-Age=2592000; HttpOnly; Secure; SameSite=None; Partitioned`.
     `SameSite=None; Partitioned` is what keeps the login working inside the fleet's frame. Locally
     (http://localhost) use `SameSite=Lax` without `Secure`/`Partitioned`.
3. **Sign the cookie** with HMAC-SHA256 keyed by the dashboard token plus another server secret (e.g. the
   data token), so rotating either logs everyone out. Check the expiry and the signature on every request.
4. **Every data endpoint** checks the cookie; with none, `401 {"error": "Not logged in: open this dashboard
   from the hivey fleet page (token login)."}`. The page shows that message instead of empty charts.
5. **Let it be framed:** no `X-Frame-Options`, and no `frame-ancestors` that excludes
   `http://127.0.0.1:*`. Turn off the host's own login wall (a Vercel project: Settings → Deployment
   Protection → off), which refuses frames; the token login protects the site instead.
6. **The secret on the server:** in code read it with `envsave.get("es_…")` (the user's
   `DASHBOARD_TOKEN_<USER>` key); on Vercel that falls back to an env var named after the key, so set it once
   per project: `envsave vercel DASHBOARD_TOKEN_<USER>` in the linked project folder (it unlocks the vault:
   tell the user which key and why, and let them run it). A plain `DASHBOARD_TOKEN` env var works too. Never write the token into the repo.

## Next.js (App Router), as in x-post-viewer

`middleware.ts`:
```ts
import { NextRequest, NextResponse } from "next/server";
export function middleware(req: NextRequest) {
  const url = req.nextUrl, token = url.searchParams.get("token");
  if (!token || url.pathname.startsWith("/api/")) return NextResponse.next();
  const rest = new URLSearchParams(url.searchParams); rest.delete("token");
  const target = new URL("/api/auth/token", url.origin);
  target.searchParams.set("token", token);
  target.searchParams.set("next", url.pathname + (rest.size ? `?${rest}` : ""));
  const res = NextResponse.redirect(target, 303);
  res.headers.set("Referrer-Policy", "no-referrer");
  return res;
}
export const config = { matcher: ["/((?!_next/|api/|favicon|.*\\.(?:png|jpg|svg|ico)$).*)"] };
```

`lib/auth.ts`:
```ts
import crypto from "node:crypto";
import envsave from "@envsave/cli";
export const SESSION_COOKIE = "session", SESSION_MAX_AGE = 60 * 60 * 24 * 30;
const want = () => envsave.get("es_…") || "";           // DASHBOARD_TOKEN_<USER>
const key = () => `${want()}\0${process.env.OTHER_SERVER_SECRET || ""}`;
const sign = (p: string) => crypto.createHmac("sha256", key()).update(p).digest("base64url");
const same = (a: string, b: string) => crypto.timingSafeEqual(
  crypto.createHash("sha256").update(a).digest(), crypto.createHash("sha256").update(b).digest());
export const tokenOk = (t: unknown) => typeof t === "string" && t.length >= 32 && want().length >= 32 && same(t, want());
export const sessionValue = () => { const exp = String(Math.floor(Date.now() / 1000) + SESSION_MAX_AGE); return `${exp}.${sign(exp)}`; };
export function loggedIn(cookie: string | undefined) {
  const [exp, sig] = (cookie || "").split(".");
  return !!exp && !!sig && Number(exp) * 1000 > Date.now() && want().length >= 32 && same(sig, sign(exp));
}
```

`app/api/auth/token/route.ts`:
```ts
import { NextRequest, NextResponse } from "next/server";
import { SESSION_COOKIE, SESSION_MAX_AGE, tokenOk, sessionValue } from "@/lib/auth";
export async function GET(req: NextRequest) {
  const url = req.nextUrl, raw = url.searchParams.get("next") || "/";
  const next = raw.startsWith("/") && !raw.startsWith("//") && !raw.startsWith("/\\") ? raw : "/";
  if (!tokenOk(url.searchParams.get("token"))) return new NextResponse("Invalid or expired dashboard token", { status: 401 });
  const res = NextResponse.redirect(new URL(next, url.origin), 303);
  const prod = process.env.NODE_ENV === "production";
  res.cookies.set(SESSION_COOKIE, sessionValue(), { httpOnly: true, sameSite: prod ? "none" : "lax", secure: prod,
    partitioned: prod, path: "/", maxAge: SESSION_MAX_AGE });
  res.headers.set("Referrer-Policy", "no-referrer"); res.headers.set("Cache-Control", "no-store");
  return res;
}
```
Data routes: `if (!loggedIn(req.cookies.get(SESSION_COOKIE)?.value)) return 401`.
A multi-user app keeps one dashboard token per user and signs the user name into the cookie.

## Static page + Vercel functions, as in trading-dashboard

- `lib/auth.js`: the same `tokenOk`, `sessionCookie()` (returns the whole `Set-Cookie` string above) and
  `loggedIn(req)` (reads `req.headers.get("cookie")`).
- `api/auth/token.js`: `export async function GET(req)`: the checks above, then
  `new Response(null, { status: 303, headers: { Location, "Set-Cookie": sessionCookie(), … } })`.
- First lines of the page script:
```js
{ const q = new URLSearchParams(location.search);
  if (q.has("token")) { const t = q.get("token"); q.delete("token");
    location.replace(`/api/auth/token?token=${encodeURIComponent(t)}&next=${encodeURIComponent(location.pathname + (q.size ? `?${q}` : ""))}`);
    throw new Error("redirecting to token login"); } }
```
- `api/data.js` returns 401 without `loggedIn(req)`.

## Check it before registering

```bash
curl -s -o /dev/null -w "%{http_code}\n" "https://<site>/api/auth/token?token=$(printf x%.0s {1..32})"   # 401
curl -sI "https://<site>/" | grep -iE "x-frame-options|frame-ancestors"                                   # nothing
```
Then register it with `--token default` and open it on the fleet page's 🖥 Dashboards tab: it should show
the dashboard logged in, inside the frame. Never print the real token while testing.
