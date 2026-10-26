//! ID-token verification against the attacks it exists to stop. Keys are generated per run
//! (P-256, ES256), so no private key is ever committed; a real Keycloak (RS256) is exercised by
//! the server's integration test.
#![expect(
    clippy::expect_used,
    reason = "token tests: failing to build a fixture is a test failure"
)]

use std::time::{SystemTime, UNIX_EPOCH};

use aws_lc_rs::{
    rand::SystemRandom,
    signature::{ECDSA_P256_SHA256_FIXED_SIGNING, EcdsaKeyPair, KeyPair as _},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use colophon_auth::{ClientSecret, OidcClient, OidcConfig, OidcError, VerifiedIdentity};
use jsonwebtoken::{Algorithm, EncodingKey, Header, jwk::JwkSet};
use serde_json::{Value, json};

const ISSUER: &str = "https://id.example.test/realms/colophon";
const CLIENT: &str = "colophon-server";
const NONCE: &str = "a-nonce-of-reasonable-length-1234";

struct Key {
    encoding: EncodingKey,
    jwk: Value,
}

fn key(kid: &str) -> Key {
    let pkcs8 =
        EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &SystemRandom::new())
            .expect("key generation");
    let pair = EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, pkcs8.as_ref())
        .expect("key parse");
    // An uncompressed P-256 point: 0x04, then 32 bytes of x and 32 of y.
    let point = pair.public_key().as_ref();
    let jwk = json!({
        "kty": "EC", "crv": "P-256", "alg": "ES256", "use": "sig", "kid": kid,
        "x": URL_SAFE_NO_PAD.encode(&point[1..33]),
        "y": URL_SAFE_NO_PAD.encode(&point[33..65]),
    });
    Key {
        encoding: EncodingKey::from_ec_der(pkcs8.as_ref()),
        jwk,
    }
}

fn now() -> i64 {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("after 1970")
        .as_secs();
    i64::try_from(seconds).expect("fits")
}

fn claims() -> Value {
    json!({
        "iss": ISSUER, "aud": CLIENT, "sub": "subject-123", "iat": now(), "exp": now() + 300,
        "nonce": NONCE, "name": "Dev Editor"
    })
}

fn with(field: &str, value: Value) -> Value {
    let mut changed = claims();
    changed[field] = value;
    changed
}

fn without(field: &str) -> Value {
    let mut changed = claims();
    changed.as_object_mut().expect("an object").remove(field);
    changed
}

fn sign(key: &Key, kid: Option<&str>, claims: &Value) -> String {
    let mut header = Header::new(Algorithm::ES256);
    header.kid = kid.map(str::to_owned);
    jsonwebtoken::encode(&header, claims, &key.encoding).expect("signing")
}

async fn client(provider: &Key) -> std::sync::Arc<OidcClient> {
    let client = OidcClient::new(OidcConfig {
        issuer: ISSUER.to_owned(),
        client_id: CLIENT.to_owned(),
        client_secret: ClientSecret::new("unused"),
        redirect_uri: "http://localhost:58000/auth/callback".to_owned(),
    })
    .expect("client");
    let set: JwkSet = serde_json::from_value(json!({ "keys": [provider.jwk] })).expect("jwks");
    client.set_keys(set).await;
    client
}

fn assert_refused(result: Result<VerifiedIdentity, OidcError>, why: &str) {
    match result {
        Err(OidcError::InvalidIdToken(_) | OidcError::Provider(_) | OidcError::Discovery(_)) => {}
        other => panic!("{why}: expected a refusal, got {other:?}"),
    }
}

#[tokio::test]
async fn a_good_token_names_the_person_by_issuer_and_subject() {
    let provider = key("provider-1");
    let client = client(&provider).await;
    let identity = client
        .verify(&sign(&provider, Some("provider-1"), &claims()), NONCE)
        .await
        .expect("valid");
    assert_eq!(identity.issuer, ISSUER);
    assert_eq!(identity.subject, "subject-123");
    assert_eq!(identity.display_name.as_deref(), Some("Dev Editor"));
}

#[tokio::test]
async fn forged_or_mismatched_tokens_are_refused() {
    let provider = key("provider-1");
    let attacker = key("provider-1");
    let client = client(&provider).await;
    let mut other_party = with("aud", json!([CLIENT, "other"]));
    other_party["azp"] = json!("other");
    let kid = Some("provider-1");

    let cases: Vec<(&str, String, &str)> = vec![
        (
            "signed by another key under the provider's key id",
            sign(&attacker, kid, &claims()),
            NONCE,
        ),
        (
            "a different nonce",
            sign(&provider, kid, &claims()),
            "another-nonce-entirely-56789",
        ),
        ("no nonce", sign(&provider, kid, &without("nonce")), NONCE),
        (
            "another issuer",
            sign(
                &provider,
                kid,
                &with("iss", json!("https://evil.example/realms/colophon")),
            ),
            NONCE,
        ),
        (
            "another audience",
            sign(&provider, kid, &with("aud", json!("someone-else"))),
            NONCE,
        ),
        (
            "expired",
            sign(&provider, kid, &with("exp", json!(now() - 3600))),
            NONCE,
        ),
        (
            "not valid yet",
            sign(&provider, kid, &with("nbf", json!(now() + 3600))),
            NONCE,
        ),
        ("no subject", sign(&provider, kid, &without("sub")), NONCE),
        (
            "several audiences and another authorized party",
            sign(&provider, kid, &other_party),
            NONCE,
        ),
        ("no key id", sign(&provider, None, &claims()), NONCE),
        (
            "an unknown key id",
            sign(&provider, Some("never-published"), &claims()),
            NONCE,
        ),
        ("garbage", "not.a.token".to_owned(), NONCE),
    ];
    for (why, token, nonce) in cases {
        assert_refused(client.verify(&token, nonce).await, why);
    }
}

#[tokio::test]
async fn symmetric_and_unsigned_tokens_are_refused() {
    let provider = key("provider-1");
    let client = client(&provider).await;
    // HS256 "signed" with the provider's public key as the HMAC secret: the classic confusion.
    let mut header = Header::new(Algorithm::HS256);
    header.kid = Some("provider-1".to_owned());
    let public = serde_json::to_vec(&provider.jwk).expect("jwk bytes");
    let hmac = jsonwebtoken::encode(&header, &claims(), &EncodingKey::from_secret(&public))
        .expect("signing");
    assert_refused(
        client.verify(&hmac, NONCE).await,
        "HS256 with the public key",
    );

    // alg "none": a header and claims with an empty signature.
    let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"none","kid":"provider-1"}"#);
    let body = URL_SAFE_NO_PAD.encode(claims().to_string());
    assert_refused(
        client.verify(&format!("{header}.{body}."), NONCE).await,
        "alg none",
    );
}
