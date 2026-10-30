# HTTP Client

Send requests from `.http` / `.rest` files (JetBrains HTTP Client and VS Code
REST Client syntax). Click **▶ Send request** above a request, or run
**HTTP: Send Request at Cursor**. The response opens beside the editor.

```http
@base = https://api.example.com

### List users
GET {{base}}/users?page=1
Accept: application/json
Authorization: Bearer {{token}}

### Create a user
# @name create
POST {{base}}/users
Content-Type: application/json

{"name": "Ann", "id": "{{$uuid}}"}

### Upload a file (relative to this .http file)
POST {{base}}/upload
Content-Type: application/json

< ./payload.json
```

- `###` separates requests; text after it names the request, `# @name x`
  too. Comments start with `#` or `//` (before the body).
- Directives: `# @no-redirect`, `# @timeout 5` (seconds; `ms`/`m` too).
- Variables: `@name = value` in the file, then the selected environment.
  Dynamic: `{{$uuid}}`, `{{$timestamp}}`, `{{$isoTimestamp}}`,
  `{{$randomInt}}`, `{{$randomInt 1 10}}`, `{{$random.integer(1, 10)}}`.
- Environments: `http-client.env.json` and `http-client.private.env.json`
  (keep secrets in the private one) next to the `.http` file or in a parent
  folder up to the project root:
  `{"$shared": {...}, "dev": {"base": "http://localhost:8080"}}`.
  Pick one with **HTTP: Select Environment** or the status-bar item.
- The panel shows status, timing and size, the body (JSON in a foldable
  viewer), headers, the raw response and the last 50 requests of this
  project. **Open in editor** opens the whole body in a new buffer.

Privacy: requests are only sent when you click; Arcode asks before the
first request to each host. The history keeps method, URL, status and
timing only, never headers or bodies. Response scripts (`> {% %}`) are not
supported: there is no JavaScript engine.
