# =============================================================
# OxiCloud — ReBAC grant management (POST/DELETE/GET /api/grants)
# =============================================================
# Exercises cross-user grants, cascading, roles, revoke, lifecycle
# cleanup. Uses ONLY endpoints that route through the
# AuthorizationEngine — handler-layer inline checks (e.g.
# GET /api/folders/{id}) are scheduled for cleanup separately.
#
# Runs AFTER permissions.hurl (bob already exists). Self-contained
# resources (unique names) so it doesn't depend on prior state.
# =============================================================


# ─────────────────────────────────────────────────────────────
# Step 1 — Login as admin (Alice), capture token + home folder.
# ─────────────────────────────────────────────────────────────
POST {{base_url}}/api/auth/login
Content-Type: application/json
{ "username": "{{username}}", "password": "{{password}}" }

HTTP 200
[Captures]
alice_token: jsonpath "$.access_token"

GET {{base_url}}/api/folders
Authorization: Bearer {{alice_token}}

HTTP 200
[Captures]
alice_home_id: jsonpath "$[0].id"


# ─────────────────────────────────────────────────────────────
# Step 2 — Create two test users specific to this file (dave + eve).
#           Avoids cross-file dependencies on bob from permissions.hurl
#           and gives us their user_id directly from the create response.
# ─────────────────────────────────────────────────────────────
POST {{base_url}}/api/admin/users
Authorization: Bearer {{alice_token}}
Content-Type: application/json
{ "username": "dave", "password": "DavePassword1!", "email": "dave@example.com", "role": "user" }

HTTP 201
[Captures]
dave_user_id: jsonpath "$.id"

POST {{base_url}}/api/admin/users
Authorization: Bearer {{alice_token}}
Content-Type: application/json
{ "username": "eve", "password": "EvePassword1!", "email": "eve@example.com", "role": "user" }

HTTP 201
[Captures]
eve_user_id: jsonpath "$.id"


# ─────────────────────────────────────────────────────────────
# Step 3 — Login dave and eve.
# ─────────────────────────────────────────────────────────────
POST {{base_url}}/api/auth/login
Content-Type: application/json
{ "username": "dave", "password": "DavePassword1!" }

HTTP 200
[Captures]
dave_token: jsonpath "$.access_token"

POST {{base_url}}/api/auth/login
Content-Type: application/json
{ "username": "eve", "password": "EvePassword1!" }

HTTP 200
[Captures]
eve_token: jsonpath "$.access_token"


# ─────────────────────────────────────────────────────────────
# Step 4 — Alice creates a folder "grant-shared" + a child "grant-child".
# ─────────────────────────────────────────────────────────────
POST {{base_url}}/api/folders
Authorization: Bearer {{alice_token}}
Content-Type: application/json
{ "name": "grant-shared", "parent_id": "{{alice_home_id}}" }

HTTP 201
[Captures]
shared_folder_id: jsonpath "$.id"

POST {{base_url}}/api/folders
Authorization: Bearer {{alice_token}}
Content-Type: application/json
{ "name": "grant-child", "parent_id": "{{shared_folder_id}}" }

HTTP 201
[Captures]
child_folder_id: jsonpath "$.id"


# ─────────────────────────────────────────────────────────────
# Step 5 — Without any grant, bob cannot rename Alice's folder.
#           PUT /api/folders/{id}/rename goes through the engine →
#           404 (anti-enumeration).
# ─────────────────────────────────────────────────────────────
PUT {{base_url}}/api/folders/{{shared_folder_id}}/rename
Authorization: Bearer {{dave_token}}
Content-Type: application/json
{ "name": "bob-tried" }

HTTP 404


# ─────────────────────────────────────────────────────────────
# Step 6 — Alice grants Bob the Viewer role. Server expands → [read].
# ─────────────────────────────────────────────────────────────
POST {{base_url}}/api/grants
Authorization: Bearer {{alice_token}}
Content-Type: application/json
{
  "subject":  { "type": "user",   "id": "{{dave_user_id}}" },
  "resource": { "type": "folder", "id": "{{shared_folder_id}}" },
  "role": "viewer"
}

HTTP 201
[Asserts]
jsonpath "$" count == 1
jsonpath "$[0].permission" == "read"


# ─────────────────────────────────────────────────────────────
# Step 7 — Viewer cannot rename (no update grant).
# ─────────────────────────────────────────────────────────────
PUT {{base_url}}/api/folders/{{shared_folder_id}}/rename
Authorization: Bearer {{dave_token}}
Content-Type: application/json
{ "name": "bob-tried-again" }

HTTP 404


# ─────────────────────────────────────────────────────────────
# Step 8 — Bob's incoming grants list contains the new grant.
# ─────────────────────────────────────────────────────────────
GET {{base_url}}/api/grants/incoming
Authorization: Bearer {{dave_token}}

HTTP 200
[Asserts]
jsonpath "$[?(@.resource.id=='{{shared_folder_id}}')].permission" == "read"


# ─────────────────────────────────────────────────────────────
# Step 9 — Promote Bob to Manager (adds comment, create, update, share).
#           PUT /api/grants/role reconciles the row set in one call.
# ─────────────────────────────────────────────────────────────
PUT {{base_url}}/api/grants/role
Authorization: Bearer {{alice_token}}
Content-Type: application/json
{
  "subject":  { "type": "user",   "id": "{{dave_user_id}}" },
  "resource": { "type": "folder", "id": "{{shared_folder_id}}" },
  "role": "manager"
}

HTTP 200
[Asserts]
jsonpath "$" count == 5


# ─────────────────────────────────────────────────────────────
# Step 10 — Bob can now rename (Manager includes update).
# ─────────────────────────────────────────────────────────────
PUT {{base_url}}/api/folders/{{shared_folder_id}}/rename
Authorization: Bearer {{dave_token}}
Content-Type: application/json
{ "name": "renamed-by-bob-as-manager" }

HTTP 200


# ─────────────────────────────────────────────────────────────
# Step 11 — Cascading: Bob can also rename the CHILD folder, because
#           his Update grant on the parent cascades via ltree to the
#           child resource — even though no direct grant on the child.
# ─────────────────────────────────────────────────────────────
PUT {{base_url}}/api/folders/{{child_folder_id}}/rename
Authorization: Bearer {{dave_token}}
Content-Type: application/json
{ "name": "renamed-child-via-cascade" }

HTTP 200


# ─────────────────────────────────────────────────────────────
# Step 12 — Bob re-shares to Carol (he has Share via Manager).
# ─────────────────────────────────────────────────────────────
POST {{base_url}}/api/grants
Authorization: Bearer {{dave_token}}
Content-Type: application/json
{
  "subject":  { "type": "user",   "id": "{{eve_user_id}}" },
  "resource": { "type": "folder", "id": "{{shared_folder_id}}" },
  "role": "viewer"
}

HTTP 201
[Captures]
eve_grant_id: jsonpath "$[0].id"


# ─────────────────────────────────────────────────────────────
# Step 13 — Carol can see the grant in her incoming list.
# ─────────────────────────────────────────────────────────────
GET {{base_url}}/api/grants/incoming
Authorization: Bearer {{eve_token}}

HTTP 200
[Asserts]
jsonpath "$[?(@.resource.id=='{{shared_folder_id}}')].permission" == "read"


# ─────────────────────────────────────────────────────────────
# Step 14 — Bob's outgoing grants list contains the grant to Carol.
# ─────────────────────────────────────────────────────────────
GET {{base_url}}/api/grants/outgoing
Authorization: Bearer {{dave_token}}

HTTP 200
[Asserts]
jsonpath "$[?(@.id=='{{eve_grant_id}}')].id" == "{{eve_grant_id}}"


# ─────────────────────────────────────────────────────────────
# Step 15 — Demote Bob to Viewer; he loses update/share/etc.
# ─────────────────────────────────────────────────────────────
PUT {{base_url}}/api/grants/role
Authorization: Bearer {{alice_token}}
Content-Type: application/json
{
  "subject":  { "type": "user",   "id": "{{dave_user_id}}" },
  "resource": { "type": "folder", "id": "{{shared_folder_id}}" },
  "role": "viewer"
}

HTTP 200
[Asserts]
jsonpath "$" count == 1
jsonpath "$[0].permission" == "read"


# ─────────────────────────────────────────────────────────────
# Step 16 — Demoted Bob can no longer rename.
# ─────────────────────────────────────────────────────────────
PUT {{base_url}}/api/folders/{{shared_folder_id}}/rename
Authorization: Bearer {{dave_token}}
Content-Type: application/json
{ "name": "bob-tried-after-demote" }

HTTP 404


# ─────────────────────────────────────────────────────────────
# Step 17 — Lifecycle: Alice deletes the folder. The DB trigger
#           trg_cleanup_grants_folder removes both bob's and carol's
#           grants automatically (also for the cascade-deleted child).
# ─────────────────────────────────────────────────────────────
DELETE {{base_url}}/api/folders/{{child_folder_id}}
Authorization: Bearer {{alice_token}}

HTTP 204

DELETE {{base_url}}/api/folders/{{shared_folder_id}}
Authorization: Bearer {{alice_token}}

HTTP 204

DELETE {{base_url}}/api/trash/empty
Authorization: Bearer {{alice_token}}

HTTP 200


# ─────────────────────────────────────────────────────────────
# Step 18 — After permanent delete, Bob's incoming list no longer
#            contains the deleted folder's grant.
# ─────────────────────────────────────────────────────────────
GET {{base_url}}/api/grants/incoming
Authorization: Bearer {{dave_token}}

HTTP 200
[Asserts]
jsonpath "$" count == 0


# ─────────────────────────────────────────────────────────────
# Step 19 — Same for Carol.
# ─────────────────────────────────────────────────────────────
GET {{base_url}}/api/grants/incoming
Authorization: Bearer {{eve_token}}

HTTP 200
[Asserts]
jsonpath "$" count == 0
