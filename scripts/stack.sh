#!/usr/bin/env bash
# Colophon's local stack (deploy/compose.yaml): PostgreSQL, Valkey, Garage and Keycloak.
#
#   scripts/stack.sh up      start everything, wait until healthy, then run the checks
#   scripts/stack.sh check   prove that each service really works
#   scripts/stack.sh down    remove the containers; keep the Postgres and Garage data
#   scripts/stack.sh reset   remove the containers and that data
#
# "Healthy" only means a container answered its own probe. The checks go further: they use
# each service the way colophon-server will, from this machine, through its published port,
# and every service also has to refuse something it must refuse.
#
# Needs docker (with Compose), curl, jq and openssl. Runs on macOS's bash 3.2.
set -euo pipefail
cd "$(dirname "$0")/.."

# Public, development-only credentials; they must match deploy/.
readonly PG_URL_IN_CONTAINER="postgresql://colophon:colophon-dev@postgres:5432/colophon"
readonly VALKEY_PASSWORD="colophon-dev"
readonly S3_ACCESS_KEY="GK636f6c6f70686f6e2d646576"
readonly S3_SECRET_KEY="636f6c6f70686f6e2d6c6f63616c2d6465762d73332d7365637265742d6b6579"
readonly S3_SIGV4="aws:amz:garage:s3"
readonly S3_BUCKET="colophon-dev"
readonly OIDC_CLIENT_ID="colophon-server"
readonly OIDC_CLIENT_SECRET="colophon-dev-client-secret"
readonly OIDC_REDIRECT_URI="http://localhost:58000/auth/callback"
readonly OIDC_USER="dev-editor"
readonly OIDC_PASSWORD="colophon-dev"

compose() { docker compose -f deploy/compose.yaml "$@"; }

checks=0
failures=0
pass() { checks=$((checks + 1)); printf '  ok    %s\n' "$1"; }
fail() { checks=$((checks + 1)); failures=$((failures + 1)); printf '  FAIL  %s\n' "$1"; }
# expect <what> <wanted> <got>
expect() {
  if [[ "$3" == "$2" ]]; then pass "$1"; else fail "$1: expected '$2', got '$3'"; fi
}

# The host:port a service's container port is published on, e.g. 127.0.0.1:55432.
published() {
  local addr
  addr=$(compose port "$1" "$2" 2>/dev/null) || return 1
  [[ -n "$addr" ]] || return 1
  printf '%s' "$addr"
}

check_postgres() {
  local addr reply out
  if ! addr=$(published postgres 5432); then fail "postgres is not running"; return; fi
  echo "postgres  $addr"

  # Send a PostgreSQL SSLRequest (length 8, code 80877103) to the published port. Only a
  # PostgreSQL server answers with the single byte 'N' (no TLS on the local stack) or 'S'.
  reply=$(
    exec 3<>"/dev/tcp/${addr%:*}/${addr##*:}" || exit 0
    printf '\000\000\000\010\004\322\026\057' >&3
    IFS= read -r -n 1 -t 5 byte <&3 || true
    printf '%s' "${byte:-}"
  )
  expect "answers the PostgreSQL protocol on the published port" "N" "$reply"

  # Connecting by service name goes through scram-sha-256, like a connection from the host;
  # the image trusts loopback connections inside the container without any password.
  out=$(compose exec -T postgres psql "${PG_URL_IN_CONTAINER/colophon-dev/wrong-password}" \
    -XAtqc 'select 1' 2>&1 || true)
  if [[ "$out" == *"password authentication failed"* ]]; then
    pass "refuses a wrong password"
  else
    fail "refuses a wrong password: got '$out'"
  fi

  out=$(compose exec -T postgres psql "$PG_URL_IN_CONTAINER" -XAtq -v ON_ERROR_STOP=1 2>&1 <<'SQL' || true
select current_setting('server_version_num')::int / 10000;
show data_checksums;
begin;
create temporary table stack_check (id int primary key, note text not null);
insert into stack_check values (1, 'colophon');
select note from stack_check where id = 1;
rollback;
SQL
  )
  expect "major version 18, data checksums on, a write reads back" \
    "18 on colophon" "$(printf '%s' "$out" | tr '\n' ' ' | sed 's/ $//')"
}

# Valkey speaks a line protocol, so the checks talk to the published port directly.
valkey_send() { printf '%s\r\n' "$1" >&4; }
valkey_line() {
  local line
  IFS= read -r -t 5 line <&4 || line="(no reply)"
  printf '%s' "${line%$'\r'}"
}
# CONFIG GET <name> replies *2, $len, name, $len, value: return the value.
valkey_config() {
  valkey_send "CONFIG GET $1"
  valkey_line >/dev/null; valkey_line >/dev/null; valkey_line >/dev/null; valkey_line >/dev/null
  valkey_line
}

check_valkey() {
  local addr key
  if ! addr=$(published valkey 6379); then fail "valkey is not running"; return; fi
  echo "valkey    $addr"
  if ! exec 4<>"/dev/tcp/${addr%:*}/${addr##*:}"; then fail "accepts a connection"; return; fi

  valkey_send "PING"
  expect "refuses commands without the password" "-NOAUTH Authentication required." "$(valkey_line)"
  valkey_send "AUTH $VALKEY_PASSWORD"
  expect "accepts the password" "+OK" "$(valkey_line)"

  key="colophon:stack-check:$$"
  valkey_send "SET $key colophon EX 30"
  valkey_line >/dev/null
  valkey_send "GET $key"
  valkey_line >/dev/null
  expect "a value with an expiry reads back" "colophon" "$(valkey_line)"
  valkey_send "DEL $key"
  valkey_line >/dev/null

  # The cache must be disposable and bounded (D-015).
  expect "memory is capped at 128 MiB" "134217728" "$(valkey_config maxmemory)"
  expect "evicts least-recently-used keys when full" "allkeys-lru" "$(valkey_config maxmemory-policy)"
  expect "never writes a snapshot" "" "$(valkey_config save)"
  expect "never writes an append-only file" "no" "$(valkey_config appendonly)"
  exec 4<&- 4>&-
}

# s3 <curl args...>: a request signed with the development key; prints the HTTP status.
s3() { curl -sS -o /dev/null -w '%{http_code}' --aws-sigv4 "$S3_SIGV4" --user "$S3_ACCESS_KEY:$S3_SECRET_KEY" "$@"; }
sha256_hex() { openssl dgst -sha256 -r | cut -d' ' -f1; }

check_garage() {
  local addr admin base tmp object
  if ! addr=$(published garage 3900) || ! admin=$(published garage 3903); then
    fail "garage is not running"; return
  fi
  echo "garage    $addr"
  base="http://$addr/$S3_BUCKET"
  object="$base/stack-check/$$.txt"
  tmp=$(mktemp -d)

  expect "cluster reports healthy" "200" "$(curl -sS -o /dev/null -w '%{http_code}' "http://$admin/health")"
  expect "the $S3_BUCKET bucket exists" "200" "$(s3 -I "$base")"

  printf 'colophon stack check %s\n' "$$" >"$tmp/in"
  # Send the payload hash explicitly: older curl releases don't add it for S3.
  expect "stores an object" "200" \
    "$(s3 -X PUT -H "x-amz-content-sha256: $(sha256_hex <"$tmp/in")" --data-binary @"$tmp/in" "$object")"
  curl -sS -o "$tmp/out" --aws-sigv4 "$S3_SIGV4" --user "$S3_ACCESS_KEY:$S3_SECRET_KEY" "$object"
  expect "returns the same bytes" "$(sha256_hex <"$tmp/in")" "$(sha256_hex <"$tmp/out" 2>/dev/null)"
  expect "refuses an anonymous read" "403" "$(curl -sS -o /dev/null -w '%{http_code}' "$object")"
  expect "refuses a request signed with the wrong secret" "403" \
    "$(curl -sS -o /dev/null -w '%{http_code}' --aws-sigv4 "$S3_SIGV4" --user "$S3_ACCESS_KEY:wrong" "$object")"
  expect "refuses an upload whose bytes don't match its SHA-256" "400" \
    "$(s3 -X PUT -H "x-amz-content-sha256: $(printf 'other bytes' | sha256_hex)" --data-binary @"$tmp/in" "$base/stack-check/$$-corrupt.txt")"
  expect "deletes it" "204" "$(s3 -X DELETE "$object")"
  expect "it is gone" "404" "$(s3 "$object")"
  rm -r "$tmp"
}

b64url() { openssl base64 -A | tr '+/' '-_' | tr -d '='; }
query_param() { sed -n "s/.*[?&]$1=\([^&]*\).*/\1/p"; }

# oidc_authorize <cookie jar> <challenge or ""> <state> <nonce>: start a sign-in
# and print the login form's action URL, or the redirect Keycloak answered with instead.
oidc_authorize() {
  local args page
  args=(--data-urlencode "client_id=$OIDC_CLIENT_ID" --data-urlencode response_type=code
    --data-urlencode scope=openid --data-urlencode "redirect_uri=$OIDC_REDIRECT_URI"
    --data-urlencode "state=$3" --data-urlencode "nonce=$4")
  if [[ -n "$2" ]]; then
    args+=(--data-urlencode "code_challenge=$2" --data-urlencode code_challenge_method=S256)
  fi
  page=$(curl -sS -c "$1" -b "$1" -w '\n%{redirect_url}' -G "$auth_endpoint" "${args[@]}")
  if [[ "$(printf '%s' "$page" | tail -n 1)" == http* ]]; then
    printf 'redirect:%s' "$(printf '%s' "$page" | tail -n 1)"
  else
    printf '%s' "$page" | grep -o 'action="[^"]*login-actions/authenticate[^"]*"' | head -n 1 |
      sed 's/^action="//; s/"$//; s/&amp;/\&/g'
  fi
}

# oidc_login <cookie jar> <form action> <password>: print where Keycloak redirects, if anywhere.
oidc_login() {
  curl -sS -o /dev/null -w '%{redirect_url}' -c "$1" -b "$1" \
    --data-urlencode "username=$OIDC_USER" --data-urlencode "password=$3" "$2"
}

# summarize <body + "\n<status> <redirect>">: the status, whether there was a redirect, and the
# first Keycloak error message on the page, so a refusal can't be mistaken for an outage.
summarize() {
  local body last
  body=$(cat)
  last=$(printf '%s' "$body" | tail -n 1)
  printf '%s %s %s' "${last%% *}" "$([[ "${last#* }" == http* ]] && echo redirect || echo no-redirect)" \
    "$(printf '%s' "$body" | grep -o 'Invalid [a-z :_]*' | head -n 1)"
}

# oidc_token <code> <verifier>: exchange a code, print the token endpoint's JSON reply.
oidc_token() {
  curl -sS "$token_endpoint" --data-urlencode grant_type=authorization_code \
    --data-urlencode "code=$1" --data-urlencode "redirect_uri=$OIDC_REDIRECT_URI" \
    --data-urlencode "code_verifier=$2" --data-urlencode "client_id=$OIDC_CLIENT_ID" \
    --data-urlencode "client_secret=$OIDC_CLIENT_SECRET"
}

check_keycloak() {
  local addr issuer discovery jar verifier challenge state nonce action location code reply payload
  if ! addr=$(published keycloak 8080); then fail "keycloak is not running"; return; fi
  echo "keycloak  $addr"
  issuer="http://localhost:${addr##*:}/realms/colophon"
  if ! discovery=$(curl -fsS "$issuer/.well-known/openid-configuration"); then
    fail "publishes the colophon realm's OpenID configuration"; return
  fi
  expect "publishes the colophon realm with a stable issuer" "$issuer" "$(jq -r .issuer <<<"$discovery")"
  auth_endpoint=$(jq -r .authorization_endpoint <<<"$discovery")
  token_endpoint=$(jq -r .token_endpoint <<<"$discovery")
  jar=$(mktemp -d)

  # A full Authorization Code + PKCE sign-in, as colophon-server will perform it in P1.
  verifier=$(openssl rand 48 | b64url)
  challenge=$(printf '%s' "$verifier" | openssl dgst -sha256 -binary | b64url)
  state=$(openssl rand 16 | b64url)
  nonce=$(openssl rand 16 | b64url)
  action=$(oidc_authorize "$jar/a" "$challenge" "$state" "$nonce")
  location=$(oidc_login "$jar/a" "$action" "$OIDC_PASSWORD")
  code=$(query_param code <<<"$location")
  if [[ -n "$code" && "$(query_param state <<<"$location")" == "$state" ]]; then
    pass "signs $OIDC_USER in and returns a code with the same state"
  else
    fail "signs $OIDC_USER in and returns a code with the same state: got '${location:-no redirect}'"
  fi
  reply=$(oidc_token "$code" "$verifier")
  # Decode the ID token's claims. Verifying its signature is the OIDC client's job (P1).
  payload=$(jq -r '.id_token // ""' <<<"$reply" | cut -d. -f2 | tr '_-' '/+')
  while (( ${#payload} % 4 )); do payload="$payload="; done
  expect "exchanges the code for an ID token for that user, with our nonce" \
    "$issuer $OIDC_CLIENT_ID $OIDC_USER true" \
    "$(openssl base64 -d -A <<<"$payload" 2>/dev/null |
      jq -r --arg n "$nonce" '"\(.iss) \(.aud) \(.preferred_username) \(.nonce == $n)"' 2>/dev/null || echo "no ID token: $reply")"
  expect "refuses the same code twice" "invalid_grant" "$(oidc_token "$code" "$verifier" | jq -r .error)"

  action=$(oidc_authorize "$jar/b" "$challenge" "$state" "$nonce")
  location=$(oidc_login "$jar/b" "$action" "$OIDC_PASSWORD")
  expect "refuses a code presented without its PKCE verifier" "invalid_grant" \
    "$(oidc_token "$(query_param code <<<"$location")" "$(openssl rand 48 | b64url)" | jq -r .error)"

  action=$(oidc_authorize "$jar/c" "$challenge" "$state" "$nonce")
  expect "refuses a wrong password" "200 no-redirect Invalid username or password" \
    "$(curl -sS -c "$jar/c" -b "$jar/c" -w '\n%{http_code} %{redirect_url}' \
      --data-urlencode "username=$OIDC_USER" --data-urlencode password=wrong-password "${action:-$auth_endpoint}" |
      summarize)"

  expect "refuses a sign-in without PKCE" "invalid_request" \
    "$(oidc_authorize "$jar/d" "" "$state" "$nonce" | query_param error)"
  expect "refuses a redirect URI that isn't registered exactly" "400 no-redirect Invalid parameter: redirect_uri" \
    "$(curl -sS -w '\n%{http_code} %{redirect_url}' -G "$auth_endpoint" \
      --data-urlencode "client_id=$OIDC_CLIENT_ID" --data-urlencode response_type=code \
      --data-urlencode scope=openid --data-urlencode "redirect_uri=http://localhost:58000/elsewhere" \
      --data-urlencode "state=$state" --data-urlencode "code_challenge=$challenge" \
      --data-urlencode code_challenge_method=S256 | summarize)"
  expect "refuses the password grant" "unauthorized_client" \
    "$(curl -sS "$token_endpoint" --data-urlencode grant_type=password \
      --data-urlencode "client_id=$OIDC_CLIENT_ID" --data-urlencode "client_secret=$OIDC_CLIENT_SECRET" \
      --data-urlencode "username=$OIDC_USER" --data-urlencode "password=$OIDC_PASSWORD" | jq -r .error)"
  rm -r "$jar"
}

check() {
  local tool
  for tool in docker curl jq openssl; do
    command -v "$tool" >/dev/null || { echo "error: $tool is required" >&2; exit 1; }
  done
  # Every check reports its own result; one broken service must not hide the others.
  set +e
  check_postgres
  check_valkey
  check_garage
  check_keycloak
  if (( failures > 0 )); then
    echo "stack check failed: $failures of $checks checks" >&2
    exit 1
  fi
  echo "stack ok: $checks checks passed"
}

case "${1:-}" in
  up)
    compose up --detach --wait --wait-timeout 300
    # Database roles for colophon-server (idempotent; see deploy/postgres/roles.sql).
    compose exec -T postgres psql -U colophon -d colophon -q -v ON_ERROR_STOP=1 <deploy/postgres/roles.sql
    check
    ;;
  check) check ;;
  down) compose down ;;
  reset) compose down --volumes ;;
  *)
    echo "usage: scripts/stack.sh up|check|down|reset" >&2
    exit 2
    ;;
esac
