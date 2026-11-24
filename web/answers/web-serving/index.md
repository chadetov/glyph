<!-- Markdown copy of https://glyphlang.io/answers/web-serving/, generated from the same source as the page. Last changed 2026-11-21. -->

17 · Serving the web

# Can I serve a web page, or only a JSON API?

A web page. `std/http` sets response headers, so a `text/html` page, a 302 redirect, a cache header, and an HTML form post are all ordinary Glyph. No Express, no hand-written TypeScript shim.

This is the router of `examples/apps/shortlink/main.glyph`, a URL shortener that ships in the repo. An HTML form creates a code, a POST answers with a redirect so a browser reload never re-posts, and following a code is a 302 to the target:

shortlink.glyph: an HTML page, a form post, and the 302

```
fn handle_home(req: Request) -> Response {
  return match req.method {
    "GET" => html(200, home_page(links.get(), query_param(req, "error"))),
    "POST" => {
      let fields = form(req)
      match create_link(form_field(fields, "url"), form_field(fields, "alias")) {
        Ok(link) => redirect(303, "/${link.code}+"),
        Err(message) => redirect(303, "/?error=${url_encode(message)}"),
      }
    },
    else => html(405, page("405", "<h1>405</h1>")),
  }
}

fn route(req: Request) -> Result<Response, string> {
  return match segments(req) {
    [] => Ok(handle_home(req)),
    [one] => match stats_code(percent_decode(one)) {
      Some(code) => Ok(handle_stats(code)),
      None => Ok(handle_code(percent_decode(one))),
    },
    else => Ok(html(404, not_found_page(path(req)))),
  }
}

async fn main(argv: Array<string>) -> number {
  let outcome = await serve(8138, route)
  return match outcome { Ok(_) => 0, Err(_) => 1, }
}
```

The same app was in the repo one release ago at 615 lines, 121 of which were a hand-written `node:http` server in a `.ts` file next to it, because a `Location` and a `text/html` body had no spelling. It is 494 lines now and imports nothing outside `std`.

## Why it matters

Until 0.1.46 a response was `{ status, body }` and the server picked the content type from the body's shape: a string was `text/plain`, anything else was JSON. That is fine for an API and useless for anything a browser renders.

The way out was to write your own server on `node:http` in a hand-written `.ts` file, which puts your whole request path outside the type checker. Two missing constructors were costing an entire application boundary.

## A handler returns a `Result`

A handler returns `Result<Response, string>`, so a 404 is an ordinary `Ok` and an `Err` is reserved for what the app cannot answer, which the server turns into a 500.

`serve` stays pending while it listens, so nothing has to keep the process alive by hand and a failed bind comes back as a value.

## Every response carries headers

`Response` carries a required `headers: Record<string, string>`. Required, not optional: every constructor fills it in, so reading `resp.headers` never needs an absence check, and the channel cannot be forgotten.

- **A new response, not a mutation** `with_header(resp, "cache-control", "no-store")` returns a new response rather than mutating one, because Glyph has no record-field mutation.
- **Checked in Glyph** The type checker knows all three constructors return a `Response`, so a handler whose declared return type says `Result<Response, string>` is checked in Glyph, not by `tsc` on generated output.

## Two details you would otherwise get wrong by hand

- **Header values** Any character Node refuses to write in a header is stripped from the value on the way out, so a `location` built from a query parameter cannot inject a second header or a second response, and an emoji in a shortened URL cannot take the server down.
- **Form bodies** `form(req)` decodes `+` as a space along with percent escapes, the pair that hand-rolled form parsing usually misses; it reads the raw body, so `req.body` is untouched for handlers that parse it themselves.

## Where it stands

### Shipping today

- `html`, `redirect`, `with_header`, `json`, `text`, and `form`
- Response headers on the wire with the content type inferred only when you did not set one
- Request headers, query parameters, path segments, and the raw body on the way in
- The client reports the response headers it received.

### On the way

- Percent encoding is the gap the snippet above shows

  **Details**

  Percent encoding is the gap the snippet above shows: `form` decodes a form body, but there is no `encode_component` for building a URL, so the shortener writes its own.

- Also missing on the client side: a timeout, a redirect policy, `head`, and the final URL a redirect landed on.
- There is no static-file server and no template engine, and there is no plan for either.

Code examples on this page are checked against Glyph 0.1.176 on every build. Last changed 21 Nov 2026.
