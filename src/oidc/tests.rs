use super::*;
use axum::{Form, Json};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD as B64};
use ring::{
    digest::{SHA256, digest},
    rand::SystemRandom,
    signature::{Ed25519KeyPair, KeyPair},
};
use serde_json::json;

#[tokio::test]
async fn login_binds_browser_validates_identity_and_revokes_session() -> Result<()> {
    // A disposable signing key and token endpoint; production HTTP clients are HTTPS-only.
    let key = Ed25519KeyPair::from_pkcs8(
        Ed25519KeyPair::generate_pkcs8(&SystemRandom::new())
            .unwrap()
            .as_ref(),
    )
    .unwrap();
    let jwks = serde_json::from_value(
        json!({"keys":[{"kty":"OKP","crv":"Ed25519","x":B64.encode(key.public_key().as_ref()),"alg":"EdDSA","kid":"test"}]}),
    )?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let token_endpoint = format!("http://{}/token", listener.local_addr()?);
    let expected = Arc::new(Mutex::new((String::new(), String::new())));
    let issue = expected.clone();
    let key = Arc::new(key);
    let provider=Router::new().route("/token",post(move |Form(fields):Form<HashMap<String,String>>| {
        let (challenge,nonce)=issue.lock().unwrap().clone();
        assert_eq!(B64.encode(digest(&SHA256,fields["code_verifier"].as_bytes())),challenge);
        let now=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
        let mut claims=json!({"sub":"owner","nonce":nonce,"iat":now,"exp":now+120,"iss":"https://id.example","aud":"lum"});
        match fields["code"].as_str(){
            "denied"=>claims["sub"]=json!("other"),
            "bad-nonce"=>claims["nonce"]=json!("different"),
            "bad-issuer"=>claims["iss"]=json!("https://other.example"),
            "bad-audience"=>claims["aud"]=json!("another-client"),
            "expired"=>claims["exp"]=json!(now-300),
            _=>(),
        }
        let message=format!("{}.{}",B64.encode(br#"{"alg":"EdDSA","kid":"test"}"#),B64.encode(claims.to_string()));
        let signature=key.sign(message.as_bytes());
        let jwt=format!("{message}.{}",B64.encode(if fields["code"]=="bad-signature" {&[0u8;64]}else{signature.as_ref()}));
        async move {Json(json!({"token_type":"Bearer","access_token":"test-only","id_token":jwt}))}
    }));
    tokio::spawn(async move { axum::serve(listener, provider).await.unwrap() });
    let meta: CoreProviderMetadata = serde_json::from_value(
        json!({"issuer":"https://id.example","authorization_endpoint":"https://id.example/authorize","token_endpoint":token_endpoint,"jwks_uri":"https://id.example/keys","response_types_supported":["code"],"subject_types_supported":["public"],"id_token_signing_alg_values_supported":["EdDSA"]}),
    )?;
    let http = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let a = Arc::new(Auth {
        cfg: Settings {
            enabled: true,
            issuer: "https://id.example".into(),
            origin: "https://lum.example".into(),
            client_id: "lum".into(),
            allowed_subjects: "owner".into(),
            ..Default::default()
        },
        secret: None,
        http: http.clone(),
        provider: tokio::sync::Mutex::new(Some((
            Instant::now() + LOGIN_TTL,
            Arc::new(meta.set_jwks(jwks)),
        ))),
        flows: Mutex::new(Entries(HashMap::new())),
        sessions: Mutex::new(Entries(HashMap::new())),
    });
    let router = routes(
        Router::new().route("/admin/runtime", get(|| async { "ok" })),
        a,
    );
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let base = format!("http://{}", l.local_addr()?);
    tokio::spawn(async move { axum::serve(l, router).await.unwrap() });
    for code in [
        "bad-state",
        "denied",
        "bad-nonce",
        "bad-issuer",
        "bad-audience",
        "expired",
        "bad-signature",
        "ok",
    ] {
        let login = http.get(format!("{base}/auth/login")).send().await?;
        let flow = login.headers()[header::SET_COOKIE]
            .to_str()?
            .split(';')
            .next()
            .unwrap();
        let url = reqwest::Url::parse(login.headers()[header::LOCATION].to_str()?)?;
        let q: HashMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(q["code_challenge_method"], "S256");
        assert_eq!(q["redirect_uri"], "https://lum.example/auth/callback");
        *expected.lock().unwrap() = (q["code_challenge"].clone(), q["nonce"].clone());
        let cb = http.get(format!("{base}/auth/callback")).query(&[
            (
                "state",
                if code == "bad-state" {
                    "wrong"
                } else {
                    &q["state"]
                },
            ),
            ("code", code),
        ]);
        assert_eq!(
            cb.try_clone().unwrap().send().await?.status(),
            StatusCode::FORBIDDEN
        );
        let r = cb
            .try_clone()
            .unwrap()
            .header(header::COOKIE, flow)
            .send()
            .await?;
        assert_eq!(
            cb.header(header::COOKIE, flow).send().await?.status(),
            StatusCode::FORBIDDEN
        );
        if code != "ok" {
            assert_eq!(r.status(), StatusCode::FORBIDDEN);
            continue;
        }
        assert_eq!(r.status(), StatusCode::SEE_OTHER);
        let cookie = r.headers()[header::SET_COOKIE].to_str()?;
        assert!(
            cookie.contains("HttpOnly")
                && cookie.contains("Secure")
                && cookie.contains("SameSite=Lax")
        );
        let session = cookie.split(';').next().unwrap();
        let admin = || {
            http.get(format!("{base}/admin/runtime"))
                .header(header::COOKIE, session)
        };
        assert_eq!(admin().send().await?.status(), StatusCode::OK);
        http.post(format!("{base}/auth/logout"))
            .header(header::COOKIE, session)
            .send()
            .await?;
        assert_eq!(admin().send().await?.status(), StatusCode::UNAUTHORIZED);
    }
    Ok(())
}
