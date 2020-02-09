use parking_lot::{Mutex, RwLock, RwLockUpgradableReadGuard};
use rand::{Rng, rngs::OsRng};

use rocket::{
    fairing::{self, Fairing, Info},
    http::{Cookie, Status},
    request::FromRequest,
    Outcome, Request, Response, Rocket, State,
};

use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt::{self, Display, Formatter};
use std::marker::PhantomData;
use std::ops::Add;
use std::time::{Duration, Instant};

/// Session store (shared state)
#[derive(Debug)]
pub struct SessionStore<D>
where
    D: 'static + Sync + Send + Default,
{
    /// The internally mutable map of sessions
    inner: RwLock<StoreInner<D>>,
    // Session config
    config: SessionConfig,
}

/// Session config object
#[derive(Debug, Clone)]
struct SessionConfig {
    /// Sessions lifespan
    lifespan: Duration,
    /// Session cookie name
    cookie_name: Cow<'static, str>,
    /// Session cookie path
    cookie_path: Cow<'static, str>,
    /// Session ID character length
    cookie_len: usize,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            lifespan: Duration::from_secs(3600),
            cookie_name: "rocket_session".into(),
            cookie_path: "/".into(),
            cookie_len: 16,
        }
    }
}

/// Mutable object stored inside SessionStore behind a RwLock
#[derive(Debug)]
struct StoreInner<D>
where
    D: 'static + Sync + Send + Default,
{
    sessions: HashMap<String, Mutex<SessionInstance<D>>>,
    last_expiry_sweep: Instant,
}

impl<D> Default for StoreInner<D>
where
    D: 'static + Sync + Send + Default,
{
    fn default() -> Self {
        Self {
            sessions: Default::default(),
            // the first expiry sweep is scheduled one lifetime from start-up
            last_expiry_sweep: Instant::now(),
        }
    }
}

/// Session, as stored in the sessions store
#[derive(Debug)]
struct SessionInstance<D>
where
    D: 'static + Sync + Send + Default,
{
    /// Data object
    data: D,
    /// Expiry
    expires: Instant,
}

/// Session ID newtype for rocket's "local_cache"
#[derive(Clone, Debug)]
struct SessionID(String);

impl SessionID {
    fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl Display for SessionID {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Session instance
///
/// To access the active session, simply add it as an argument to a route function.
///
/// Sessions are started, restored, or expired in the `FromRequest::from_request()` method
/// when a `Session` is prepared for one of the route functions.
#[derive(Debug)]
pub struct Session<'a, D>
where
    D: 'static + Sync + Send + Default,
{
    /// The shared state reference
    store: State<'a, SessionStore<D>>,
    /// Session ID
    id: &'a SessionID,
}

impl<'a, 'r, D> FromRequest<'a, 'r> for Session<'a, D>
where
    D: 'static + Sync + Send + Default,
{
    type Error = ();

    fn from_request(request: &'a Request<'r>) -> Outcome<Self, (Status, Self::Error), ()> {
        let store: State<SessionStore<D>> = request.guard().unwrap();
        Outcome::Success(Session {
            id: request.local_cache(|| {
                let store_ug = store.inner.upgradable_read();

                // Resolve session ID
                let id = if let Some(cookie) = request.cookies().get(&store.config.cookie_name) {
                    Some(SessionID(cookie.value().to_string()))
                } else {
                    None
                };

                let expires = Instant::now().add(store.config.lifespan);

                if let Some(m) = id
                    .as_ref()
                    .and_then(|token| store_ug.sessions.get(token.as_str()))
                {
                    // --- ID obtained from a cookie && session found in the store ---

                    let mut inner = m.lock();
                    if inner.expires <= Instant::now() {
                        // Session expired, reuse the ID but drop data.
                        inner.data = D::default();
                    }

                    // Session is extended by making a request with valid ID
                    inner.expires = expires;

                    id.unwrap()
                } else {
                    // --- ID missing or session not found ---

                    // Get exclusive write access to the map
                    let mut store_wg = RwLockUpgradableReadGuard::upgrade(store_ug);

                    // This branch runs less often, and we already have write access,
                    // let's check if any sessions expired. We don't want to hog memory
                    // forever by abandoned sessions (e.g. when a client lost their cookie)

                    // Throttle by lifespan - e.g. sweep every hour
                    if store_wg.last_expiry_sweep.elapsed() > store.config.lifespan {
                        let now = Instant::now();
                        store_wg.sessions.retain(|_k, v| v.lock().expires > now);

                        store_wg.last_expiry_sweep = now;
                    }

                    // Find a new unique ID - we are still safely inside the write guard
                    let new_id = SessionID(loop {
                        let token: String = OsRng
                            .sample_iter(&rand::distributions::Alphanumeric)
                            .take(store.config.cookie_len)
                            .collect();

                        if !store_wg.sessions.contains_key(&token) {
                            break token;
                        }
                    });

                    store_wg.sessions.insert(
                        new_id.to_string(),
                        Mutex::new(SessionInstance {
                            data: Default::default(),
                            expires,
                        }),
                    );

                    new_id
                }
            }),
            store,
        })
    }
}

impl<'a, D> Session<'a, D>
where
    D: 'static + Sync + Send + Default,
{
    /// Create the session fairing.
    ///
    /// You can configure the session store by calling chained methods on the returned value
    /// before passing it to `rocket.attach()`
    pub fn fairing() -> SessionFairing<D> {
        SessionFairing::<D>::new()
    }

    /// Clear session data (replace the value with default)
    pub fn clear(&self) {
        self.tap(|m| {
            *m = D::default();
        })
    }

    /// Access the session's data using a closure.
    ///
    /// The closure is called with the data value as a mutable argument,
    /// and can return any value to be is passed up to the caller.
    pub fn tap<T>(&self, func: impl FnOnce(&mut D) -> T) -> T {
        // Use a read guard, so other already active sessions are not blocked
        // from accessing the store. New incoming clients may be blocked until
        // the tap() call finishes
        let store_rg = self.store.inner.read();

        // Unlock the session's mutex.
        // Expiry was checked and prolonged at the beginning of the request
        let mut instance = store_rg
            .sessions
            .get(self.id.as_str())
            .expect("Session data unexpectedly missing")
            .lock();

        func(&mut instance.data)
    }
}

/// Fairing struct
#[derive(Default)]
pub struct SessionFairing<D>
where
    D: 'static + Sync + Send + Default,
{
    config: SessionConfig,
    phantom: PhantomData<D>,
}

impl<D> SessionFairing<D>
where
    D: 'static + Sync + Send + Default,
{
    fn new() -> Self {
        Self::default()
    }

    /// Set session lifetime (expiration time).
    ///
    /// Call on the fairing before passing it to `rocket.attach()`
    pub fn with_lifetime(mut self, time: Duration) -> Self {
        self.config.lifespan = time;
        self
    }

    /// Set session cookie name and length
    ///
    /// Call on the fairing before passing it to `rocket.attach()`
    pub fn with_cookie_name(mut self, name: impl Into<Cow<'static, str>>) -> Self {
        self.config.cookie_name = name.into();
        self
    }

    /// Set session cookie name and length
    ///
    /// Call on the fairing before passing it to `rocket.attach()`
    pub fn with_cookie_len(mut self, length: usize) -> Self {
        self.config.cookie_len = length;
        self
    }

    /// Set session cookie name and length
    ///
    /// Call on the fairing before passing it to `rocket.attach()`
    pub fn with_cookie_path(mut self, path: impl Into<Cow<'static, str>>) -> Self {
        self.config.cookie_path = path.into();
        self
    }
}

impl<D> Fairing for SessionFairing<D>
where
    D: 'static + Sync + Send + Default,
{
    fn info(&self) -> Info {
        Info {
            name: "Session",
            kind: fairing::Kind::Attach | fairing::Kind::Response,
        }
    }

    fn on_attach(&self, rocket: Rocket) -> Result<Rocket, Rocket> {
        // install the store singleton
        Ok(rocket.manage(SessionStore::<D> {
            inner: Default::default(),
            config: self.config.clone(),
        }))
    }

    fn on_response<'r>(&self, request: &'r Request, response: &mut Response) {
        // send the session cookie, if session started
        let session = request.local_cache(|| SessionID("".to_string()));

        if !session.0.is_empty() {
            response.adjoin_header(
                Cookie::build(self.config.cookie_name.clone(), session.to_string())
                    .path("/")
                    .finish(),
            );
        }
    }
}
