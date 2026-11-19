# Coexistence proxy (dev and harness only after the Phase 8 cutover)

> **Phase 8.** The shipped proxy (`apps/proxy/Caddyfile.ce`, and the AIO
> variant) no longer imports this fragment. It routes `/api/*`, `/auth/*` and
> `/static/*` straight to `plane-api:8000`; `PLANE_RS_GROUP_*`,
> `route-groups.yaml` and `Caddyfile.coexist` are retired from production.
> They stay in the tree because the differential harness and
> `apps/server/dev/proxy.sh` still need them: the dev proxy compares Rust and
> Django side by side on the same port (`dev/Caddyfile.dev` imports the
> generated fragment), and `tools/contract` and `tools/proxy/test_gen_caddy.py`
> use the same ownership table and group flags. Do not copy
> `Caddyfile.coexist` into a production image.

Caddy decides, per request, whether Django or the Rust `plane-api` answers.
The decision is generated, not hand-written, so moving a route group is a
one-line change plus a reload.

| File | Who writes it | What it is |
| --- | --- | --- |
| `route-groups.yaml` (`deploy/`) | by hand | Which routes belong to which group. |
| `ROUTES.md` (Owner column) | by hand, preserved by `routes.py` | Which methods of a route are Rust's. |
| `rust-routes.caddy` (this directory) | `tools/proxy/gen_caddy.py` | Generated Caddyfile fragment: one `handle` block per Rust group. |
| `Caddyfile.coexist` (this directory) | by hand | `apps/proxy/Caddyfile.ce` plus the import and a fallback `handle`. |
| `../dev/Caddyfile.dev` | by hand | The local proxy on 18080 (see `dev/README.md`). |

`apps/proxy/Caddyfile.ce` itself is not modified: production keeps using it
until cutover. `Caddyfile.coexist` is the drop-in replacement for the
transition, and `apps/server/dev/proxy.sh` runs the same generated fragment
against the local stack. The dev script writes its own gitignored copy,
`dev/run/rust-routes.caddy`, and never the committed `deploy/caddy/` file.

## Moving a group

1. In `apps/server/ROUTES.md`, change the route's Owner cell from `django` to
   `rust` (whole row, including implicit `HEAD`/`OPTIONS`) or to
   `rust(GET,HEAD)` (only those methods). Method names are uppercase and must
   be methods the row answers; invalid cells are rejected by both
   `tools/inventory/routes.py` and `tools/proxy/gen_caddy.py`. Only a path the
   production proxy already sends to the API can move: the generator parses the
   `reverse_proxy … api:8000` lines of `apps/proxy/Caddyfile.ce` (`/api/*`,
   `/auth/*`, `/static/*` today) and refuses a `rust` cell outside them — `/`
   and `/robots.txt` go to the web frontend, so `health` and `web` stay on
   Django by construction.
2. Regenerate the inventory (it preserves Owner and Status):

   ```sh
   cd apps/api
   DJANGO_SETTINGS_MODULE=plane.settings.production SECRET_KEY=inventory \
   DATABASE_URL=postgresql://x:x@127.0.0.1:1/x REDIS_URL=redis://127.0.0.1:1/0 \
   ENABLE_DRF_SPECTACULAR=0 .venv/bin/python ../server/tools/inventory/routes.py
   ```

3. Regenerate the proxy fragment and check it:

   ```sh
   apps/api/.venv/bin/python apps/server/tools/proxy/gen_caddy.py
   apps/api/.venv/bin/python apps/server/tools/proxy/gen_caddy.py --check
   ```

4. Reload the proxy. Development/harness: `apps/server/dev/proxy.sh reload`
   regenerates the gitignored `dev/run/rust-routes.caddy` from the same inputs
   and reloads Caddy; it does not write the committed fragment. Production
   stopped using `Caddyfile.coexist` at the Phase 8 cutover (see the note at
   the top); there is nothing to flip there.

`gen_caddy.py --explain` prints every route with its group and effective
methods, which is the quickest way to see what a change did.

## Flags

Every group has one variable, `PLANE_RS_GROUP_<NAME>` (`-` becomes `_`,
upper-cased; for example `users-me` becomes `PLANE_RS_GROUP_USERS_ME`). The
generated file lists the groups and their flags at the top.

- Unset or `rust`: the group's Rust-owned paths and methods go to
  `PLANE_RS_API_UPSTREAM` (default `plane-api:8000`).
- `django`: every method of the group goes back to Django, even when ROUTES.md
  says `rust`. Any other value also goes to Django (fail safe, not fail open).

The value is read **per request** with Caddy's `{env.*}` placeholder, not
substituted at config load with `{$...}`. The generated file is therefore
identical in every environment, and the same bytes work whether a deployment
wants a group on Rust or back on Django. The process environment cannot change
while Caddy runs, so flipping a flag means restarting (or recreating) the
proxy; `caddy reload` alone is enough only after regenerating
`rust-routes.caddy` because ownership changed.

A group with no Rust-owned route emits no matcher, so its flag does nothing
until a route in it is marked `rust`.

## Matching details

For each group the generator emits:

```caddyfile
@plane_rs_<group> {
	expression <<CEL
		({env.PLANE_RS_GROUP_<GROUP>} == "" ? "rust" : {env.PLANE_RS_GROUP_<GROUP>}) == "rust"
		&& (
			(method("GET", "HEAD", "OPTIONS") && (path_regexp(r"^/api/...$") || ...))
			|| ...
		)
	CEL
}
handle @plane_rs_<group> {
	reverse_proxy {$PLANE_RS_API_UPSTREAM:plane-api:8000}
}
```

- Paths are anchored `^...$` RE2 regexes built from Django's converter regexes
  (`apps/api/.venv/lib/python3.12/site-packages/django/urls/converters.py`;
  uuid is lowercase-only), DRF's `.format` regex, and DRF's optional trailing
  slash. `path_regexp` matches the URI-decoded path after Caddy's `cleanPath`
  (double slashes merged, dot segments removed); Django's resolver also works
  on the decoded `PATH_INFO`, so they agree. A request such as
  `/api//instances/` is matched here as `/api/instances/` where Django would
  404; the Rust router must 404 it too.
- Methods are matched exactly. A whole-row `rust` includes the implicit `HEAD`
  Django answers for a `GET` and the `OPTIONS` DRF answers for every DRF view.
  `rust(GET,HEAD)` moves only those methods; the same path can therefore be
  split between the two backends.
- Multiple `(method(...) && path_regexp(...))` clusters are OR-ed, because two
  routes in one group can have different method sets. That is what keeps a
  `rust(GET)` on one path from sending the other path's `POST` to Rust.
- `handle` blocks are mutually exclusive and are not reordered by the
  Caddyfile adapter, so the Rust handles are evaluated before the plain
  `reverse_proxy /api/*` and `/auth/*` rules, and the fallback `handle` in
  `Caddyfile.coexist` is skipped once a Rust handle matches. A bare
  `reverse_proxy @matcher` would be sorted after the path-specific directives
  and would also let the request continue down the chain, proxying it twice.
- `/` and `/robots.txt` keep going to `web:3000` through the fallback
  `reverse_proxy /*` unless their Owner cells are moved.

## Validating

The repository CI runs the generator in `--check` mode and the Python tests in
`tools/proxy/test_gen_caddy.py`. To validate a real Caddy config (the official
image, no containers left behind):

```sh
docker run --rm \
  -e SITE_ADDRESS=:8080 -e BUCKET_NAME=uploads -e FILE_SIZE_LIMIT=52428800 \
  -v "$PWD/apps/server/deploy/caddy:/etc/caddy" \
  caddy:2 caddy validate --config /etc/caddy/Caddyfile.coexist --adapter caddyfile
```

`SITE_ADDRESS`, `BUCKET_NAME` and `FILE_SIZE_LIMIT` are the same variables the
production image sets; without them the site address is empty and the file
cannot be adapted.

## Rollback

Set `PLANE_RS_GROUP_<NAME>=django` for the affected group and restart the
proxy. That is the whole rollback: no Rust redeploy, no Caddyfile edit.
